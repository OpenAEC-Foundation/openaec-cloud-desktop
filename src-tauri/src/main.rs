// Verberg het console-venster op Windows in release-builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    openaec_cloud_lib::run()
}
