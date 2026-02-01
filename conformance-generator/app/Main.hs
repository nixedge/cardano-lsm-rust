{-# LANGUAGE RecordWildCards #-}

module Main where

import ConformanceGen
import Data.Aeson (encode)
import qualified Data.ByteString.Lazy as BL
import System.FilePath ((</>), (<.>))
import System.Directory (createDirectoryIfMissing)
import System.Environment (getArgs)
import Control.Monad (forM)
import Text.Printf (printf)
import Text.Read (readMaybe)

data Options = Options
  { outputDir :: FilePath
  , numTests :: Int
  , maxOps :: Int
  , seedStart :: Int
  , verbose :: Bool
  } deriving (Show)

defaultOptions :: Options
defaultOptions = Options
  { outputDir = "conformance-tests"
  , numTests = 100
  , maxOps = 1000
  , seedStart = 1
  , verbose = False
  }

parseArgs :: [String] -> Options
parseArgs = go defaultOptions
  where
    go opts [] = opts
    go opts ("--output":dir:rest) = go (opts { outputDir = dir }) rest
    go opts ("-o":dir:rest) = go (opts { outputDir = dir }) rest
    go opts ("--num-tests":n:rest) = 
      case readMaybe n of
        Just num -> go (opts { numTests = num }) rest
        Nothing -> go opts rest
    go opts ("-n":n:rest) = 
      case readMaybe n of
        Just num -> go (opts { numTests = num }) rest
        Nothing -> go opts rest
    go opts ("--max-ops":n:rest) = 
      case readMaybe n of
        Just num -> go (opts { maxOps = num }) rest
        Nothing -> go opts rest
    go opts ("-m":n:rest) = 
      case readMaybe n of
        Just num -> go (opts { maxOps = num }) rest
        Nothing -> go opts rest
    go opts ("--seed-start":n:rest) = 
      case readMaybe n of
        Just num -> go (opts { seedStart = num }) rest
        Nothing -> go opts rest
    go opts ("-s":n:rest) = 
      case readMaybe n of
        Just num -> go (opts { seedStart = num }) rest
        Nothing -> go opts rest
    go opts ("--verbose":rest) = go (opts { verbose = True }) rest
    go opts ("-v":rest) = go (opts { verbose = True }) rest
    go opts ("--help":_) = error usage
    go opts ("-h":_) = error usage
    go opts (_:rest) = go opts rest

usage :: String
usage = unlines
  [ "conformance-generator - Generate LSM tree conformance tests"
  , ""
  , "Usage: conformance-generator [OPTIONS]"
  , ""
  , "Options:"
  , "  -o, --output DIR        Output directory (default: conformance-tests)"
  , "  -n, --num-tests N       Number of tests (default: 100)"
  , "  -m, --max-ops N         Max operations per test (default: 1000)"
  , "  -s, --seed-start N      Starting seed (default: 1)"
  , "  -v, --verbose           Verbose output"
  , "  -h, --help              Show this help"
  ]

main :: IO ()
main = do
  args <- getArgs
  let opts = parseArgs args
  
  putStrLn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  putStrLn "  Cardano LSM Tree - Conformance Test Generator"
  putStrLn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  putStrLn ""
  putStrLn $ "Generating " ++ show (numTests opts) ++ " test cases"
  putStrLn $ "Output directory: " ++ outputDir opts
  putStrLn ""
  
  -- Create output directory
  createDirectoryIfMissing True (outputDir opts)
  
  -- Generate test cases
  results <- forM [seedStart opts .. seedStart opts + numTests opts - 1] $ \seed ->
    generateAndWrite opts seed
  
  let (succeeded, failed) = partition snd results
  
  putStrLn ""
  putStrLn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  putStrLn $ "✅ Generated: " ++ show (length succeeded)
  putStrLn $ "📁 Output: " ++ outputDir opts
  putStrLn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

partition :: (a -> Bool) -> [a] -> ([a], [a])
partition pred = foldr select ([], [])
  where
    select x (ts, fs)
      | pred x = (x : ts, fs)
      | otherwise = (ts, x : fs)

generateAndWrite :: Options -> Int -> IO (Int, Bool)
generateAndWrite Options{..} seed = do
  let progress = seed - seedStart + 1
  
  when (verbose || progress `mod` 10 == 0) $
    printf "  [%3d/%3d] test_%d\n" progress numTests seed
  
  -- Vary size slightly
  let numOps = min maxOps ((seed * 7) `mod` maxOps + 10)
  
  -- Generate and run
  testCase <- generateTestCase seed numOps 10
  expectedResults <- runAndRecord testCase
  
  -- Write files
  let testFile = outputDir </> ("test_" ++ show seed) <.> "json"
  let expectedFile = outputDir </> ("test_" ++ show seed) <.> "expected" <.> "json"
  
  BL.writeFile testFile (encode testCase)
  BL.writeFile expectedFile (encode expectedResults)
  
  return (seed, True)

when :: Bool -> IO () -> IO ()
when True action = action
when False _ = return ()
