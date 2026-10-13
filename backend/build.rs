// `sqlx::migrate!` embeds ./migrations at compile time; make sure adding or
// editing a migration triggers a rebuild so the binary never ships stale.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
