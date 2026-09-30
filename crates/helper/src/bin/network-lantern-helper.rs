fn main() {
    if let Err(error) = dispatch() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn dispatch() -> lantern_helper::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    #[cfg(windows)]
    match args.as_slice() {
        [mode] if mode == "--service" => lantern_helper::HelperServer::platform_default()?.run(),
        [mode, owner_sid] if mode == "--install-service" => {
            lantern_helper::HelperServer::install_platform_service(owner_sid)
        }
        [mode] if mode == "--remove-service" => {
            lantern_helper::HelperServer::remove_platform_service()
        }
        _ => Err(lantern_helper::HelperError::Validation(
            "expected --service, --install-service <owner SID>, or --remove-service".into(),
        )),
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "linux")]
        match args.as_slice() {
            [] => lantern_helper::HelperServer::platform_default()?.run(),
            [mode] if mode == "--cleanup-credentials" => {
                lantern_helper::cleanup_platform_credentials()
            }
            _ => Err(lantern_helper::HelperError::Validation(
                "expected no arguments or --cleanup-credentials".into(),
            )),
        }
        #[cfg(not(target_os = "linux"))]
        {
            if !args.is_empty() {
                return Err(lantern_helper::HelperError::Validation(
                    "this helper service accepts no command-line arguments on this platform".into(),
                ));
            }
            lantern_helper::HelperServer::platform_default()?.run()
        }
    }
}
