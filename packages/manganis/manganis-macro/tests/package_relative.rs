use manganis::{Asset, asset};

#[test]
fn an_asset_in_its_package_is_recorded_relative_to_it() {
    const ASSET: Asset = asset!("/assets/asset.txt");
    let bundled = ASSET.bundled();
    let package = concat!(env!("CARGO_PKG_NAME"), "@", env!("CARGO_PKG_VERSION"));
    assert_eq!(
        bundled.absolute_source_path(),
        format!("cargo-pkg:{package}/assets/asset.txt")
    );
    assert_eq!(
        bundled.package_relative_source(),
        Some((package, "assets/asset.txt"))
    );
}
