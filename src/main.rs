//! The `btm-mod` command line; see `btm_mod::cli`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = btm_mod::cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
