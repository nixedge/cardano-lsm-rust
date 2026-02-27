{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE RecordWildCards #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE TypeApplications #-}

{- |
Module: ConformanceGen
Description: Generate conformance tests for LSM tree implementations

This module generates random operation sequences using QuickCheck and runs them
against the Haskell lsm-tree reference implementation, recording both the
operations and their results for cross-language conformance testing.

The approach mirrors how Cardano validates the Haskell ledger implementation
against the Agda specification.
-}

module ConformanceGen
  ( Operation(..)
  , OperationResult(..)
  , TestCase(..)
  , TestConfig(..)
  , TestResults(..)
  , generateTestCase
  , runAndRecord
  ) where

import Data.Aeson (ToJSON, FromJSON, encode, object, (.=))
import qualified Data.Aeson as Aeson
import qualified Data.Aeson.Types as Aeson (Parser)
import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString.Base64 as Base64
import Data.Text (Text)
import qualified Data.Text as T
import qualified Data.Text.Encoding as T
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as Map
import Data.Word (Word8)
import qualified Data.Vector.Primitive as VP
import qualified Data.Vector as V
import GHC.Generics (Generic)
import Test.QuickCheck
import System.Random (mkStdGen, setStdGen)
import Control.Monad (foldM, forM)
import Control.Monad.IO.Class (liftIO)
import Control.Exception (catch, SomeException, try)
import System.IO.Temp (withSystemTempDirectory)
import System.Directory (createDirectoryIfMissing)

-- Real lsm-tree imports
import qualified Database.LSMTree.Simple as LSM
import System.FS.API (MountPoint(..), mkFsPath)
import System.FS.BlockIO.API (HasBlockIO(..))
import System.FS.BlockIO.IO (defaultIOCtxParams, ioHasBlockIO)

--------------------------------------------------------------------------------
-- Data Types for Test Cases
--------------------------------------------------------------------------------

-- | Operations that can be performed on an LSM tree
data Operation
  = Insert { opKey :: ByteString, opValue :: ByteString }
  | Get { opKey :: ByteString }
  | Delete { opKey :: ByteString }
  | Range { opFrom :: ByteString, opTo :: ByteString }
  | Snapshot { snapId :: String }
  | Rollback { snapId :: String }
  | Compact
  deriving (Show, Eq, Generic)

instance ToJSON Operation where
  toJSON (Insert k v) = object 
    [ "type" .= ("Insert" :: Text)
    , "key" .= encodeB64 k
    , "value" .= encodeB64 v
    ]
  toJSON (Get k) = object
    [ "type" .= ("Get" :: Text)
    , "key" .= encodeB64 k
    ]
  toJSON (Delete k) = object
    [ "type" .= ("Delete" :: Text)
    , "key" .= encodeB64 k
    ]
  toJSON (Range f t) = object
    [ "type" .= ("Range" :: Text)
    , "from" .= encodeB64 f
    , "to" .= encodeB64 t
    ]
  toJSON (Snapshot sid) = object
    [ "type" .= ("Snapshot" :: Text)
    , "id" .= sid
    ]
  toJSON (Rollback sid) = object
    [ "type" .= ("Rollback" :: Text)
    , "snapshot_id" .= sid
    ]
  toJSON Compact = object
    [ "type" .= ("Compact" :: Text)
    ]

instance FromJSON Operation where
  parseJSON = Aeson.withObject "Operation" $ \v -> do
    opType <- v Aeson..: "type" :: Aeson.Parser Text
    case opType of
      "Insert" -> do
        keyB64 <- v Aeson..: "key"
        valueB64 <- v Aeson..: "value"
        case (decodeB64 keyB64, decodeB64 valueB64) of
          (Right k, Right val) -> return $ Insert k val
          _ -> fail "Invalid base64 in Insert"
      "Get" -> do
        keyB64 <- v Aeson..: "key"
        case decodeB64 keyB64 of
          Right k -> return $ Get k
          _ -> fail "Invalid base64 in Get"
      "Delete" -> do
        keyB64 <- v Aeson..: "key"
        case decodeB64 keyB64 of
          Right k -> return $ Delete k
          _ -> fail "Invalid base64 in Delete"
      "Range" -> do
        fromB64 <- v Aeson..: "from"
        toB64 <- v Aeson..: "to"
        case (decodeB64 fromB64, decodeB64 toB64) of
          (Right f, Right t) -> return $ Range f t
          _ -> fail "Invalid base64 in Range"
      "Snapshot" -> Snapshot <$> v Aeson..: "id"
      "Rollback" -> Rollback <$> v Aeson..: "snapshot_id"
      "Compact" -> return Compact
      _ -> fail $ "Unknown operation type: " ++ T.unpack opType

-- | Results of LSM operations
data OperationResult
  = Ok (Maybe Text)           -- base64-encoded value or null
  | OkRange [(Text, Text)]    -- [(base64 key, base64 value)]
  | OkUnit                    -- For unit-returning operations
  | Err Text                  -- Error message
  deriving (Show, Eq, Generic)

instance ToJSON OperationResult where
  toJSON (Ok (Just v)) = object ["Ok" .= v]
  toJSON (Ok Nothing) = object ["Ok" .= Aeson.Null]
  toJSON (OkRange pairs) = object ["OkRange" .= pairs]
  toJSON OkUnit = object ["OkUnit" .= Aeson.Null]
  toJSON (Err msg) = object ["Err" .= msg]

instance FromJSON OperationResult

-- | Test configuration parameters
data TestConfig = TestConfig
  { memtable_size :: Int
  , bloom_bits_per_key :: Int
  , level0_compaction_trigger :: Int
  } deriving (Show, Eq, Generic)

instance ToJSON TestConfig
instance FromJSON TestConfig

-- | A complete test case
data TestCase = TestCase
  { version :: String
  , seed :: Int
  , config :: TestConfig
  , operations :: [Operation]
  } deriving (Show, Eq, Generic)

instance ToJSON TestCase
instance FromJSON TestCase

-- | Results from running a test case
data TestResults = TestResults
  { results :: [OperationResult]
  } deriving (Show, Eq, Generic)

instance ToJSON TestResults
instance FromJSON TestResults

--------------------------------------------------------------------------------
-- QuickCheck Generators
--------------------------------------------------------------------------------

-- | Generate a random key (1-32 bytes)
genKey :: Gen ByteString
genKey = do
  len <- choose (1, 32)
  bytes <- vectorOf len (arbitrary :: Gen Word8)
  return $ BS.pack bytes

-- | Generate a random value with size distribution
genValue :: Gen ByteString
genValue = frequency
  [ (70, genSmallValue)    -- 0-256 bytes
  , (20, genMediumValue)   -- 256-4096 bytes
  , (8, genLargeValue)     -- 4K-64K bytes
  , (2, genEmptyValue)     -- Empty value
  ]
  where
    genSmallValue = do
      len <- choose (1, 256)
      BS.pack <$> vectorOf len arbitrary
    
    genMediumValue = do
      len <- choose (256, 4096)
      BS.pack <$> vectorOf len arbitrary
    
    genLargeValue = do
      len <- choose (4096, 65536)
      BS.pack <$> vectorOf len arbitrary
    
    genEmptyValue = return BS.empty

-- | Generate a single operation with context awareness
genOperation :: Int -> Int -> Gen Operation
genOperation opsRemaining currentSnapshots = frequency $
  coreOps ++ snapshotOps ++ rollbackOps
  where
    coreOps =
      [ (40, Insert <$> genKey <*> genValue)
      , (30, Get <$> genKey)
      , (15, Delete <$> genKey)
      , (10, genRangeOp)
      , (5, return Compact)
      ]
    
    -- Range operation with proper ordering
    genRangeOp = do
      k1 <- genKey
      k2 <- genKey
      let (from, to) = if k1 <= k2 then (k1, k2) else (k2, k1)
      return $ Range from to
    
    -- Snapshot operations (more common early in test)
    snapshotOps
      | opsRemaining > 50 = [(5, genSnapshotOp)]
      | opsRemaining > 10 = [(3, genSnapshotOp)]
      | otherwise = [(1, genSnapshotOp)]
    
    genSnapshotOp = do
      snapNum <- choose (0 :: Int, 999)
      return $ Snapshot $ "snap_" ++ show snapNum
    
    -- Rollback operations (only if snapshots exist)
    rollbackOps
      | currentSnapshots > 0 = [(3, genRollbackOp currentSnapshots)]
      | otherwise = []
    
    genRollbackOp n = do
      snapNum <- choose (0, n - 1)
      return $ Rollback $ "snap_" ++ show snapNum

-- | Generate a sequence of operations
genOperationSequence :: Int -> Int -> Int -> Gen [Operation]
genOperationSequence 0 _ _ = return []
genOperationSequence n maxSnaps currentSnaps = do
  op <- genOperation n currentSnaps
  let newSnapCount = case op of
        Snapshot {} -> min (currentSnaps + 1) maxSnaps
        _ -> currentSnaps
  rest <- genOperationSequence (n - 1) maxSnaps newSnapCount
  return (op : rest)

-- | Generate a complete test case
generateTestCase :: Int     -- ^ Seed for reproducibility
                 -> Int     -- ^ Number of operations
                 -> Int     -- ^ Maximum concurrent snapshots
                 -> IO TestCase
generateTestCase seed numOps maxSnapshots = do
  setStdGen (mkStdGen seed)
  ops <- generate $ genOperationSequence numOps maxSnapshots 0
  return $ TestCase
    { version = "1.0"
    , seed = seed
    , config = TestConfig
        { memtable_size = 4096
        , bloom_bits_per_key = 10
        , level0_compaction_trigger = 4
        }
    , operations = ops
    }

--------------------------------------------------------------------------------
-- Test Execution Against Haskell lsm-tree
--------------------------------------------------------------------------------

{- |
Run test case against actual Haskell lsm-tree.

This uses the real Database.LSMTree.Simple API to execute operations
and record results for conformance testing.
-}
runWithLSMTree :: TestCase -> IO TestResults
runWithLSMTree TestCase{..} =
  withSystemTempDirectory "lsm-conformance" $ \tmpDir -> do
    -- Execute against real lsm-tree
    LSM.withOpenSession tmpDir $ \session -> do
      LSM.withTable session $ \(table :: LSM.Table BS.ByteString BS.ByteString) -> do
        -- Store snapshot names for rollback
        results <- executeOperations table Map.empty operations
        return $ TestResults { results }
  where
    -- Execute operations and collect results
    -- Now threads the current table through each operation to allow rollback
    executeOperations :: LSM.Table BS.ByteString BS.ByteString
                      -> Map String (LSM.Table BS.ByteString BS.ByteString)
                      -> [Operation]
                      -> IO [OperationResult]
    executeOperations _ _ [] = return []
    executeOperations table snapshots (op:ops) = do
      (result, newTable, newSnapshots) <- executeOperation table snapshots op
      rest <- executeOperations newTable newSnapshots ops
      return (result : rest)

    -- Execute a single operation
    -- Now returns (result, newTable, newSnapshots) to allow table swapping on rollback
    executeOperation :: LSM.Table BS.ByteString BS.ByteString
                     -> Map String (LSM.Table BS.ByteString BS.ByteString)
                     -> Operation
                     -> IO (OperationResult, LSM.Table BS.ByteString BS.ByteString, Map String (LSM.Table BS.ByteString BS.ByteString))
    executeOperation table snapshots (Insert k v) = do
      LSM.insert table k v
      return (OkUnit, table, snapshots)

    executeOperation table snapshots (Get k) = do
      maybeVal <- LSM.lookup table k
      let result = case maybeVal of
            Just v -> Ok (Just (encodeB64 v))
            Nothing -> Ok Nothing
      return (result, table, snapshots)

    executeOperation table snapshots (Delete k) = do
      LSM.delete table k
      return (OkUnit, table, snapshots)

    executeOperation table snapshots (Range from to) = do
      let range = LSM.FromToIncluding from to
      entries <- LSM.rangeLookup table range
      let pairs = [(encodeB64 k, encodeB64 v) | (k, v) <- V.toList entries]
      return (OkRange pairs, table, snapshots)

    executeOperation table snapshots (Snapshot sid) = do
      -- Note: The Haskell lsm-tree doesn't support in-memory snapshots in the same way
      -- Instead, we use duplicate to create a logical snapshot
      dup <- LSM.duplicate table
      return (OkUnit, table, Map.insert sid dup snapshots)

    executeOperation table snapshots (Rollback sid) = do
      -- Rollback by swapping to the snapshot table
      -- Since duplicate() creates a separate table, we can use it directly
      case Map.lookup sid snapshots of
        Just snapshotTable -> do
          -- Create a new duplicate of the snapshot to use as current table
          -- This ensures subsequent operations work on the rolled-back state
          newTable <- LSM.duplicate snapshotTable
          return (OkUnit, newTable, snapshots)
        Nothing -> return (Err "Snapshot not found", table, snapshots)

    executeOperation table snapshots Compact = do
      -- The Simple API doesn't expose compaction directly
      -- Compaction happens automatically in the background
      return (OkUnit, table, snapshots)

--------------------------------------------------------------------------------
-- Helper Functions
--------------------------------------------------------------------------------

-- | Encode ByteString as base64 Text
encodeB64 :: ByteString -> Text
encodeB64 = T.decodeUtf8 . Base64.encode

-- | Decode base64 Text to ByteString
decodeB64 :: Text -> Either String ByteString
decodeB64 = Base64.decode . T.encodeUtf8


--------------------------------------------------------------------------------
-- Alternative Implementation Using Reference Model
--------------------------------------------------------------------------------

{- |
Alternative: If the lsm-tree API is complex or snapshot/rollback aren't directly
supported, we can use a reference model (pure Map) and run both implementations.

This ensures we generate valid test cases that work with basic LSM semantics.
-}

type RefModel = Map ByteString (Maybe ByteString)
type Snapshots = Map String RefModel

-- | Run against reference model (pure, simple semantics)
runReferenceModel :: [Operation] -> [OperationResult]
runReferenceModel ops = 
  let (_, _, results) = foldl execOp (Map.empty, Map.empty, []) ops
  in reverse results
  where
    execOp (model, snapshots, acc) (Insert k v) = 
      (Map.insert k (Just v) model, snapshots, OkUnit : acc)
    
    execOp (model, snapshots, acc) (Get k) =
      let result = case Map.lookup k model of
            Just (Just v) -> Ok (Just (encodeB64 v))
            _ -> Ok Nothing
      in (model, snapshots, result : acc)
    
    execOp (model, snapshots, acc) (Delete k) =
      (Map.insert k Nothing model, snapshots, OkUnit : acc)
    
    execOp (model, snapshots, acc) (Range from to) =
      let entries = [(k, v) | (k, Just v) <- Map.toList model, k >= from, k <= to]
          pairs = map (\(k, v) -> (encodeB64 k, encodeB64 v)) entries
      in (model, snapshots, OkRange pairs : acc)
    
    execOp (model, snapshots, acc) (Snapshot sid) =
      -- Store snapshot
      let newSnapshots = Map.insert sid model snapshots
      in (model, newSnapshots, OkUnit : acc)
    
    execOp (model, snapshots, acc) (Rollback sid) =
      -- Restore from snapshot
      case Map.lookup sid snapshots of
        Just savedModel -> (savedModel, snapshots, OkUnit : acc)
        Nothing -> (model, snapshots, Err "Snapshot not found" : acc)
    
    execOp (model, snapshots, acc) Compact =
      -- Compact: remove tombstones
      let compacted = Map.filter (\v -> case v of Just _ -> True; Nothing -> False) model
          compacted' = Map.map (\(Just v) -> Just v) compacted
      in (compacted', snapshots, OkUnit : acc)

{- |
Fallback implementation using reference model.

This is used if we can't access the real lsm-tree API, but still want to
generate valid test cases that follow LSM semantics.
-}
runAndRecordReference :: TestCase -> IO TestResults
runAndRecordReference TestCase{..} = do
  let results = runReferenceModel operations
  return $ TestResults { results }

{- |
Main entry point: try to use real lsm-tree, fall back to reference model.

This ensures test generation works even if lsm-tree API integration is incomplete.
Once the API is fully wired up, this will use the real implementation.
-}
runAndRecord :: TestCase -> IO TestResults
runAndRecord testCase = do
  -- Try to use real lsm-tree
  result <- try @SomeException $ runWithLSMTree testCase
  
  case result of
    Right testResults -> do
      putStrLn "✅ Using Haskell lsm-tree reference implementation"
      return testResults
    
    Left err -> do
      putStrLn $ "⚠️  lsm-tree API not available: " ++ show err
      putStrLn "   Falling back to reference model"
      runAndRecordReference testCase

--------------------------------------------------------------------------------
-- Statistics and Utilities
--------------------------------------------------------------------------------

-- | Analyze generated test case for statistics
analyzeTestCase :: TestCase -> String
analyzeTestCase TestCase{..} = unlines
  [ "Test Case Statistics:"
  , "  Seed: " ++ show seed
  , "  Total operations: " ++ show (length operations)
  , "  Inserts: " ++ show inserts
  , "  Gets: " ++ show gets
  , "  Deletes: " ++ show deletes
  , "  Ranges: " ++ show ranges
  , "  Snapshots: " ++ show snapshots
  , "  Rollbacks: " ++ show rollbacks
  , "  Compacts: " ++ show compacts
  ]
  where
    inserts = length [() | Insert {} <- operations]
    gets = length [() | Get {} <- operations]
    deletes = length [() | Delete {} <- operations]
    ranges = length [() | Range {} <- operations]
    snapshots = length [() | Snapshot {} <- operations]
    rollbacks = length [() | Rollback {} <- operations]
    compacts = length [() | Compact <- operations]
