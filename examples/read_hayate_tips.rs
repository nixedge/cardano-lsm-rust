use cardano_lsm::{LsmTree, LsmConfig, Key};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <path-to-hayate-db>", args[0]);
        println!("Example: {} .run/sanchonet/hayate/sanchonet/chain_tip", args[0]);
        return Ok(());
    }
    
    let db_path = PathBuf::from(&args[1]);
    println!("Opening LSM tree at: {}", db_path.display());
    
    let config = LsmConfig::default();
    let tree = LsmTree::open(db_path, config)?;
    
    // Try to get the global chain tip
    let global_tip_key = Key::from(b"current_tips");
    if let Some(value) = tree.get(&global_tip_key)? {
        println!("\n✓ Found global chain tip:");
        let tip_str = String::from_utf8_lossy(value.as_ref());
        println!("  {}", tip_str);
    } else {
        println!("\n✗ No global chain tip found");
    }
    
    // Search for wallet tips by trying common patterns
    let wallet_id = "acct_xvk10lmrmwnjep30tt0k8dy33a75vr90qjk9jvejk8atp6m3yyrp00mzcv4x3g4sq9wn50hpfjgq2a5a6qlnx84dpt086z08wlwva5n6ahs4x2g7w";
    
    println!("\n🔍 Searching for wallet tips:");
    println!("  Wallet ID: {}", wallet_id);
    println!("  Wallet ID length: {}", wallet_id.len());
    
    // Try exact key
    let exact_key_str = format!("wallet_tip:{}", wallet_id);
    let exact_key = Key::from(exact_key_str.as_bytes());
    println!("\n  Trying exact key (length {}):", exact_key_str.len());
    
    if let Some(value) = tree.get(&exact_key)? {
        println!("  ✓ Found wallet tip with exact key!");
        let tip_str = String::from_utf8_lossy(value.as_ref());
        println!("  {}", tip_str);
    } else {
        println!("  ✗ No wallet tip found with exact key");
    }
    
    // Try variations with extra characters
    for suffix in &['r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z'] {
        let variant_key_str = format!("{}{}", exact_key_str, suffix);
        let variant_key = Key::from(variant_key_str.as_bytes());
        
        if let Some(value) = tree.get(&variant_key)? {
            println!("\n  ✓ Found wallet tip with suffix '{}'!", suffix);
            println!("  Key length: {}", variant_key_str.len());
            let tip_str = String::from_utf8_lossy(value.as_ref());
            println!("  {}", tip_str);
        }
    }
    
    Ok(())
}
