use std::fs;
use std::path::Path;

fn fix_imports(path: &Path) {
    if let Ok(content) = fs::read_to_string(path) {
        let fixed = content
            .replace("// alloy::prelude removed, import manually", "// alloy::prelude removed, import manually")
            .replace("use revm_interpreter::Evm", "use revm_interpreter::Evm")
            .replace("use revm_database::db", "use revm_database::db")
            .replace("use revm_primitives::AccountInfo", "use revm_primitives::AccountInfo")
            .replace("use revm_primitives::ExecutionResult", "use revm_primitives::ExecutionResult")
            .replace("use revm_primitives::TransactTo", "use revm_primitives::TransactTo")
            .replace("use revm_bytecode::Bytecode", "use revm_bytecode::Bytecode")
            .replace("use alloy_provider::Provider", "use alloy_provider::Provider")
            .replace("use alloy_provider::RootProvider", "use alloy_provider::RootProvider")
            .replace("use alloy_provider::ProviderBuilder", "use alloy_provider::ProviderBuilder")
            .replace("use alloy_provider::IpcConnect", "use alloy_provider::IpcConnect")
            .replace("use alloy_provider::ext", "use alloy_provider::ext");
        
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
