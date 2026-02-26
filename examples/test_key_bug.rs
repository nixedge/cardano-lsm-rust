use cardano_lsm::{LsmTree, LsmConfig, Key, Value};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    // Create a test LSM tree
    let test_dir = PathBuf::from("/tmp/lsm-test");
    std::fs::create_dir_all(&test_dir)?;
    
    // Clean up any existing data
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir)?;
    
    let config = LsmConfig::default();
    let mut tree = LsmTree::open(test_dir.clone(), config)?;
    
    // The exact wallet ID from hayate config
    let wallet_id = "acct_xvk10lmrmwnjep30tt0k8dy33a75vr90qjk9jvejk8atp6m3yyrp00mzcv4x3g4sq9wn50hpfjgq2a5a6qlnx84dpt086z08wlwva5n6ahs4x2g7w";
    let key_str = format!("wallet_tip:{}", wallet_id);
    
    println!("Input wallet_id length: {}", wallet_id.len());
    println!("Input key_str: {}", key_str);
    println!("Input key_str length: {}", key_str.len());
    println!("Input key_str (last 20 bytes): {:?}", &key_str.as_bytes()[key_str.len().saturating_sub(20)..]);
    
    // Insert the key
    let key = Key::from(key_str.as_bytes());
    let value = Value::from(b"{\"slot\":12345,\"hash\":\"abcd\"}");
    tree.insert(&key, &value)?;
    
    // Flush to disk
    drop(tree);
    
    // Now check what was written to the WAL
    let wal_path = test_dir.join("wal.log");
    if wal_path.exists() {
        let wal_contents = std::fs::read(&wal_path)?;
        println!("\nWAL file size: {} bytes", wal_contents.len());
        
        // Search for "wallet_tip" in the WAL
        let search = b"wallet_tip:";
        if let Some(pos) = wal_contents.windows(search.len()).position(|w| w == search) {
            println!("Found 'wallet_tip:' at position: {}", pos);
            
            // Extract the full key from WAL
            let start = pos;
            let mut end = start;
            while end < wal_contents.len() && wal_contents[end] >= 32 && wal_contents[end] < 127 {
                end += 1;
            }
            
            let stored_key = String::from_utf8_lossy(&wal_contents[start..end]);
            println!("Stored key in WAL: {}", stored_key);
            println!("Stored key length: {}", end - start);
            println!("Stored key (last 20 bytes): {:?}", &wal_contents[end.saturating_sub(20)..end]);
        }
    }
    
    Ok(())
}
