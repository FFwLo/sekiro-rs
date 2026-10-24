//! What another crate (RustSekiro) loads a map with, each item named through the library, so
//! making one private again fails this build (#193).

#[test]
fn the_map_library_items_stay_public() {
    use sekiro_rs::map;
    let _: fn(&mut bevy::prelude::App) = map::ready::install;
    let _ = map::upload_budget;
    let _ = map::layered_gltf_plugin;
    let _ = map::world::enabled;
    let _ = map::world::connects;
    let _ = map::world::play_blocks;
    let _ = map::world::collision_world;
    let _ = map::visibility::supported;
    let _: Option<&map::story::MapStory> = None;
    let _: Option<map::AppHook> = map::APP_HOOKS.get().map(|h| h.0);
}

/// A host's own command line never reaches the library: until `set_args` gives it flags, every
/// flag reads as absent, and the first `set_args` wins.
#[test]
fn a_host_gives_the_library_its_flags() {
    use sekiro_rs::map::world::connects;
    assert!(connects("m11_00_00_00"));
    assert!(sekiro_rs::set_args(["--no-connected-blocks"]));
    assert!(!connects("m11_00_00_00"));
    assert!(!sekiro_rs::set_args(Vec::<String>::new()));
}

/// The load-timing clock starts at a host's first `mark_process_start` and stays there.
#[test]
fn a_host_starts_the_load_clock_once() {
    let start = sekiro_rs::mark_process_start();
    assert_eq!(sekiro_rs::mark_process_start(), start);
}
