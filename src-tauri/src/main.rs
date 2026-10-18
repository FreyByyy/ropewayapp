// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `ropeway status`, `ropeway send …` and other terminal commands.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let args: Vec<String> = std::env::args().collect();
        if ropeway_lib::cli::is_cli(&args) {
            std::process::exit(ropeway_lib::cli::main(args));
        }
    }
    ropeway_lib::run()
}
