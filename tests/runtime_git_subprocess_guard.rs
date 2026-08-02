use std::fs;
use std::path::Path;

#[test]
fn production_git_modules_do_not_launch_the_git_executable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in ["src/enforce.rs", "src/query.rs", "src/git.rs"] {
        let source = fs::read_to_string(root.join(path)).unwrap();
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(
            !production.contains("Command::new(\"git\")"),
            "{path} launches the git executable outside test-only code"
        );
    }
}
