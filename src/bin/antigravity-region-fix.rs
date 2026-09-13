fn main() {
    if let Err(error) =
        antigravity_region_fix::runtime::manage(std::env::args_os().skip(1).collect())
    {
        eprintln!("antigravity-region-fix: {error:#}");
        std::process::exit(1);
    }
}
