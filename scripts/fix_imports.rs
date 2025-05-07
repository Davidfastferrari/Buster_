use std::fs::{self, read_to_string};
use std::path::Path;
use std::io::Write;

fn main() {
    let src_path = "./"; // Change if you want to limit to src folders
    walk_and_fix(Path::new(src_path));
}

fn walk_and_fix(dir: &Path) {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk_and_fix(&path);
            } else if path.extension().map_or(false, |e| e == "rs") {
                fix_file(&path);
            }
        }
    }
}

fn fix_file(file_path: &Path) {
    let content = read_to_string(file_path).expect("Failed to read file");
    let fixed = content
        // Alloy corrections windows command to run fix first: rustc scripts\fix_imports.rs -o fix_imports.exe  second: fix_imports.exe
        // .replace("alloy::rpc::client::Client", "alloy::rpc::client::Client")
        // .replace("alloy::rpc::types", "alloy::rpc::types")
         .replace("alloy::providers::Provider", "alloy::providers::Provider");
        // .replace("alloy::primitives::Address", "alloy::primitives::Address")
        //    .replace("use alloy_primitives", "use alloy_primitives")
        //     .replace("use alloy_sol_types", "use alloy_sol_types")
        //     .replace("use alloy_rpc_types", "use alloy_rpc_types")
        //     .replace("use alloy_rpc_client", "use alloy_rpc_client")
        //     .replace("use alloy::providers::Provider", "use alloy::providers::Provider")
        //     .replace("use alloy_network", "use alloy_network")
        //     .replace("use alloy_node_bindings", "use alloy_node_bindings")
        //     .replace("use alloy_eips", "use alloy_eips")
        // Reth corrections
        // .replace("reth::providers::Provider", "reth::providers::Provider")
        // .replace("reth_db::database::Database", "reth_db::database::Database")
        // .replace("reth::executor", "reth::executor")
        // REVM corrections
        // .replace("revm::db::Database", "revm::db::Database")
        // .replace("revm::primitives::Bytecode", "revm::primitives::Bytecode")
        // .replace("revm::primitives::Address", "revm::primitives::Address");

    if fixed != content {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(file_path)
            .expect("Failed to open file for writing");
        file.write_all(fixed.as_bytes()).expect("Failed to write file");
        println!("Fixed imports in: {:?}", file_path);
    }
}
