// Standalone native fake for assisted-launch tests. It has no messenger code,
// network calls or profile access. Output stays next to its test-only binary.
fn main() {
    let here = std::env::current_exe().unwrap();
    std::fs::write(here.parent().unwrap().join("fake-client-launched.txt"),
        format!("arguments={}", std::env::args_os().skip(1).count())).unwrap();
}
