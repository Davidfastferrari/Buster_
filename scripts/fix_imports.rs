use std::fs;
use std::path::Path;

fn fix_imports(path: &Path) {
    if let Ok(content) = fs::read_to_string(path) {
        let fixed = content
            // Revm fixes
        .replace("use revm::interpreter::Evm;", "use revm::interpreter::Evm;")
        .replace("use revm::interpreter::Evm;", "use revm::interpreter::Evm;")
        .replace("use revm::primitives::execution_result::ExecutionResult;", "use revm::primitives::execution_result::ExecutionResult;")
        .replace("use revm::primitives::transact_to::TransactTo;", "use revm::primitives::transact_to::TransactTo;")
        .replace("use revm::database::*;", "use revm::database::*;")
        .replace("use revm::database::*;", "use revm::database::*;")
        .replace("// TODO: inspector_handle_register was removed. Refactor to new Inspector API!", "// TODO: inspector_handle_register was removed. Refactor to new Inspector API!")
        // --- Alloy provider wrong usage (OPTIONAL placeholder if needed later) ---
        .replace("use alloy_provider::Provider;", "use alloy_provider::Provider;")
        .replace("use alloy_provider::ProviderBuilder;", "use alloy_provider::ProviderBuilder;")
        .replace("use alloy_provider::RootProvider;", "use alloy_provider::RootProvider;")
        .replace("use alloy_provider::IpcConnect;", "use alloy_provider::IpcConnect;")
        .replace("use alloy_provider::ext;", "use alloy_provider::ext;");
        if fixed != content {
            println!("Fixing {:?}", path);
            fs::write(path, fixed).expect("Failed to write file");
        }
    }
}

fn walk_dirs(dir: &Path) {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).expect("Cannot read directory") {
            let entry = entry.expect("Cannot get entry");
            let path = entry.path();
            if path.is_dir() {
                walk_dirs(&path);
            } else if path.extension().map_or(false, |ext| ext == "rs") {
                fix_imports(&path);
            }
        }
    }
}

fn main() {
    walk_dirs(Path::new("./"));
}
