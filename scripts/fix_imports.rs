use std::fs;
use std::io::{self, Write};
use std::path::Path;

fn main() -> io::Result<()> {
    let root_dir = "src"; // Your Rust project root

    let mappings = [
        // --- REVM Fixes ---
        ("use revm::Evm;", "use revm::interpreter::Evm;"),
        ("use revm_interpreter::Evm;", "use revm::interpreter::Evm;"),
        ("use revm::primitives::ExecutionResult;", "use revm::primitives::execution_result::ExecutionResult;"),
        ("use revm::primitives::TransactTo;", "use revm::primitives::transact_to::TransactTo;"),
        ("use revm::db;", "use revm::database::*;"),
        ("use revm_database::db;", "use revm::database::*;"),
        ("use revm::inspector_handle_register;", "// TODO: inspector_handle_register was removed. Refactor to new Inspector API!"),

        // --- Alloy provider wrong usage (OPTIONAL placeholder if needed later) ---
        ("use alloy_provider::Provider::Provider;", "use alloy_provider::Provider;"),
        ("use alloy_provider::Provider::ProviderBuilder;", "use alloy_provider::ProviderBuilder;"),
        ("use alloy_provider::Provider::RootProvider;", "use alloy_provider::RootProvider;"),
        ("use alloy_provider::Provider::IpcConnect;", "use alloy_provider::IpcConnect;"),
        ("use alloy_provider::Provider::ext;", "use alloy_provider::ext;"),
    ];

    process_dir(root_dir, &mappings)?;

    Ok(())
}

fn process_dir<P: AsRef<Path>>(path: P, mappings: &[(&str, &str)]) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            process_dir(&path, mappings)?;
        } else if path.extension().map_or(false, |ext| ext == "rs") {
            fix_file(&path, mappings)?;
        }
    }
    Ok(())
}

fn fix_file<P: AsRef<Path>>(file_path: P, mappings: &[(&str, &str)]) -> io::Result<()> {
    let file_path = file_path.as_ref();
    let content = fs::read_to_string(file_path)?;

    let mut new_content = content.clone();
    for (old, new) in mappings {
        new_content = new_content.replace(old, new);
    }

    if content != new_content {
        let mut file = fs::File::create(file_path)?;
        file.write_all(new_content.as_bytes())?;
        println!("✅ Fixed: {:?}", file_path);
    }

    Ok(())
}
