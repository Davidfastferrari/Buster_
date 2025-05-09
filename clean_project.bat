@echo off
echo 🛠️  Cleaning Rust Project... Let's go!

cargo fix --edition --allow-dirty
cargo clippy --fix --allow-dirty --allow-staged -Zunstable-options
cargo clean
cargo build

echo ✅ Project is now auto-cleaned and fresh!
pause
