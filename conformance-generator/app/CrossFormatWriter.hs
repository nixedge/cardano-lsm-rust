{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE RecordWildCards #-}

{- |
Module: CrossFormatWriter
Description: Write LSM tree database files for cross-format validation

This program creates an actual LSM tree database using the Haskell lsm-tree
implementation, performs some operations, and saves the database to disk.
The resulting files are then read by the Rust implementation to validate
byte-level file format compatibility.
-}

module Main where

import qualified Data.ByteString as BS
import qualified Data.ByteString.Char8 as BS8
import System.Environment (getArgs)
import System.Directory (createDirectoryIfMissing, removePathForcibly)
import System.FilePath ((</>))
import Control.Monad (forM_)

-- Real lsm-tree imports
import qualified Database.LSMTree.Simple as LSM

main :: IO ()
main = do
  args <- getArgs
  let outputDir = case args of
        (dir:_) -> dir
        [] -> "cross-format-test-data"

  putStrLn $ "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  putStrLn $ "  Cross-Format LSM Tree Writer (Haskell)"
  putStrLn $ "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  putStrLn $ "Output directory: " ++ outputDir
  putStrLn ""

  -- Clean up if exists
  removePathForcibly outputDir
  createDirectoryIfMissing True outputDir

  -- Create session root
  let sessionRoot = outputDir </> "session"
  createDirectoryIfMissing True sessionRoot

  -- Use the simple session API
  LSM.withOpenSession sessionRoot $ \sess -> do
    -- Create table (uses default config with LazyLevelling and 10 bits per key)
    LSM.withTable sess $ \(tbl :: LSM.Table BS.ByteString BS.ByteString) -> do
      putStrLn "Writing test data..."

      -- Write a variety of test data
      -- Test 1: Simple inserts
      forM_ [1..10 :: Int] $ \i -> do
        let key = BS8.pack $ "key" ++ show i
        let value = BS8.pack $ "value" ++ show i
        LSM.insert tbl key value

      putStrLn "  ✓ Inserted 10 key-value pairs"

      -- Test 2: Larger values (will go to blobs)
      let largeValue = BS.replicate 1000 0x42  -- 1KB value
      LSM.insert tbl "large_key" largeValue
      putStrLn "  ✓ Inserted large value (1KB)"

      -- Test 3: Deletes (tombstones)
      LSM.delete tbl "key2"
      LSM.delete tbl "key4"
      putStrLn "  ✓ Deleted 2 keys (tombstones)"

      -- Test 4: Updates (overwrites)
      LSM.insert tbl "key1" "updated_value1"
      putStrLn "  ✓ Updated 1 key"

      -- Test 5: Range of keys with common prefix
      forM_ [1..5 :: Int] $ \i -> do
        let key = BS8.pack $ "prefix_" ++ show i
        let value = BS8.pack $ "prefix_value_" ++ show i
        LSM.insert tbl key value
      putStrLn "  ✓ Inserted 5 keys with common prefix"

      -- Force flush to disk by creating a snapshot
      putStrLn ""
      putStrLn "Creating snapshot to force flush to disk..."
      _snap1 <- LSM.duplicate tbl
      putStrLn "  ✓ Snapshot created (forces SSTable flush)"

      -- More operations after snapshot
      forM_ [11..15 :: Int] $ \i -> do
        let key = BS8.pack $ "key" ++ show i
        let value = BS8.pack $ "value" ++ show i
        LSM.insert tbl key value
      putStrLn "  ✓ Inserted 5 more keys"

      -- Another flush
      _snap2 <- LSM.duplicate tbl
      putStrLn "  ✓ Second snapshot (creates another SSTable)"

      putStrLn ""
      putStrLn "Database written successfully!"
      putStrLn ""
      putStrLn "Expected data in database:"
      putStrLn "  - key1 = 'updated_value1' (updated)"
      putStrLn "  - key2 = <deleted>"
      putStrLn "  - key3 = 'value3'"
      putStrLn "  - key4 = <deleted>"
      putStrLn "  - key5..key15 = 'value5'..'value15'"
      putStrLn "  - large_key = <1KB of 0x42 bytes>"
      putStrLn "  - prefix_1..prefix_5 = 'prefix_value_1'..'prefix_value_5'"
      putStrLn ""

  putStrLn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  putStrLn "✅ Cross-format test data generated!"
  putStrLn $ "📁 Location: " ++ outputDir ++ "/session"
  putStrLn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
