fn main() {
    println!("cargo:rerun-if-changed=migrations/0001_init.sql");
    println!("cargo:rerun-if-changed=migrations/0002_add_payload_and_df.sql");
    println!("cargo:rerun-if-changed=migrations/0003_trace.sql");
    println!("cargo:rerun-if-changed=migrations/0004_scan.sql");
    tauri_build::build()
}
