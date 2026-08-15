fn main() {
    println!("cargo:rerun-if-changed=migrations/0001_init.sql");
    println!("cargo:rerun-if-changed=migrations/0002_add_payload_and_df.sql");
    tauri_build::build()
}
