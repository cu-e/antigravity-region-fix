fn main() {
    if let Err(error) =
        antigravity_region_fix::runtime::launch(std::env::args_os().skip(1).collect())
    {
        eprintln!("pagy: {error:#}");
        std::process::exit(1);
    }
}
