fn main() {
    #[cfg(target_os = "linux")]
    match tgsum_runner::egress::claude_relay(std::env::args_os().skip(1).collect()) {
        Ok(code) => std::process::exit(code),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(125);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("Claude relay is not qualified on this OS");
        std::process::exit(125);
    }
}
