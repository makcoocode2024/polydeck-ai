@echo off
echo === PolyDeck Build ===
echo.
echo [1/4] Installing npm dependencies...
call npm ci
echo.
echo [2/4] Running Rust tests...
cargo test --workspace
echo.
echo [3/4] Running frontend tests...
call npm test
echo.
echo [4/4] Building Tauri app...
call npx tauri build
echo.
echo === Build Complete ===
