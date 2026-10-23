//! `--map <id> (--camera x,y,z,yaw,pitch | --at-idol <entity>) --rustsekiro <dir>`: RustSekiro's combat (the `rustsekiro`
//! crate, https://github.com/FFwLo/RustSekiro, built as its core with the `embed` feature) on the
//! map. Wolf and RustSekiro's configured enemy stand and fight on the play world's collision,
//! under the map's lighting, with RustSekiro's own camera, lock-on, turn speeds and HUD (click to
//! capture the mouse, Esc gives it back; F1 its debug menu). `<dir>` is a RustSekiro folder: its
//! `extracted/` (RustSekiro's extractor) and `config.toml` (the enemy, Wolf's level). Wolf starts
//! on the floor under the camera, facing its yaw, with the config's enemy 8 m ahead; or at an
//! idol's warp point (BonfireWarpParam `bonfireEntityId`, e.g. 1101956 Ashina Outskirts on
//! m11_00_00_00) in the open world, with the map's own enemies around him.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::{Result, anyhow};
use bevy::prelude::*;

/// Where Wolf starts: under the camera (x, y, z, yaw in degrees) or at an idol.
#[derive(Clone)]
enum Start {
    Camera([f32; 4]),
    Idol(i32),
}

/// The map and Wolf's start, from the command line.
static START: OnceLock<(String, Start)> = OnceLock::new();
/// The play world's map blocks (the open world's enemies come per block).
static BLOCKS: OnceLock<Vec<String>> = OnceLock::new();
/// The play world's Sculptor's Idols by bonfire entity: base on the floor, the way it faces.
static IDOLS: OnceLock<std::collections::HashMap<i64, (Vec3, Vec3)>> = OnceLock::new();
/// The command line's RustSekiro folder (a Travel to another map starts this exe again).
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sets the map mode up to run the combat (`map::APP_HOOKS`).
pub fn install(args: &[String], dir: PathBuf) -> Result<()> {
    anyhow::ensure!(
        dir.join("extracted").is_dir(),
        "--rustsekiro {}: no extracted/ folder (run RustSekiro's extractor)",
        dir.display()
    );
    let map = crate::arg_value(args, "--map").ok_or_else(|| anyhow!("--rustsekiro needs --map <id>"))?;
    let start = match crate::arg_value(args, "--at-idol") {
        Some(e) => Start::Idol(e.parse().map_err(|e| anyhow!("--at-idol wants an idol entity id: {e}"))?),
        None => match crate::map::parse_camera(crate::arg_value(args, "--camera").as_deref())? {
            Some([x, y, z, yaw, _]) => Start::Camera([x, y, z, yaw]),
            None => anyhow::bail!("--rustsekiro needs --camera x,y,z,yaw,pitch or --at-idol <entity> (where Wolf starts)"),
        },
    };
    if let Start::Idol(_) = start {
        // An idol start is the open world: no duel line-up; the map's own enemies (its MSB
        // characters and events, rustsekiro enemy/world.rs) stream in around Wolf.
        if std::env::var_os("SHINOBI_WORLD_ENEMIES").is_none() {
            // Before any other thread reads the environment.
            unsafe { std::env::set_var("SHINOBI_WORLD_ENEMIES", "1") };
        }
        rustsekiro::set_map_hooks(rustsekiro::MapHooks { idol_start, open_world, blocks, ..rustsekiro::NO_MAP_HOOKS });
    }
    rustsekiro::set_idol_hooks(rustsekiro::IdolHooks { world: open_world, spot: idol_spot, can_travel: |_| true, start: idol_start, travel, rested });
    let _ = DIR.set(dir.clone());
    rustsekiro::set_root(dir);
    let _ = START.set((map, start));
    let _ = crate::map::APP_HOOKS.set((before, after));
    Ok(())
}

/// The open world's hooks (an idol start).
fn idol_start() -> Option<i64> {
    match START.get() {
        Some((_, Start::Idol(e))) => Some(*e as i64),
        _ => None,
    }
}

/// The idols of the play blocks' layouts: each object whose entity is a BonfireWarpParam
/// `bonfireEntityId` (play/idol.rs `Idols::place`), its front the object's -Z.
fn idols(layouts: &std::collections::BTreeMap<String, serde_json::Value>, world: &sekiro_sim::collision::CollisionWorld) -> std::collections::HashMap<i64, (Vec3, Vec3)> {
    let cache = sekiro_sim::cache::root();
    let warps: std::collections::HashSet<i64> = sekiro_sim::registry::rows(&cache, "BonfireWarpParam")
        .map(|rows| rows.values().filter_map(|r| r["bonfireEntityId"].as_i64()).filter(|e| *e > 0).collect())
        .unwrap_or_default();
    let mut out = std::collections::HashMap::new();
    for o in layouts.values().flat_map(|l| l["objects"].as_array().into_iter().flatten()) {
        let Some(e) = o["entity_id"].as_i64().filter(|e| warps.contains(e)) else { continue };
        let Some(pose) = crate::map::pose(o) else { continue };
        let base = pose.translation;
        let base = base.with_y(world.floor(base.x, base.z, base.y + 1.0).unwrap_or(base.y));
        let front = (pose.rotation.normalize() * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::Z);
        out.insert(e, (base, front));
    }
    info!("rustsekiro: {} Sculptor's Idols in the play world", out.len());
    out
}

fn idol_spot(entity: i64) -> Option<(Vec3, Vec3)> {
    IDOLS.get()?.get(&entity).copied()
}

/// Travel to an idol of another map: this exe again on that map at the idol; this run ends.
fn travel(entity: i64) {
    // Bonfire entity AABCCCC: map mAA_BB_00_00.
    let map = format!("m{:02}_{:02}_00_00", entity / 100_000, (entity / 10_000) % 10);
    let dir = DIR.get().cloned().unwrap_or_default();
    info!("rustsekiro: travel to idol {entity} on {map}");
    let started = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .args(["--map", &map, "--at-idol", &entity.to_string(), "--rustsekiro"])
            .arg(&dir)
            .spawn()
    });
    match started {
        Ok(_) => std::process::exit(0),
        Err(e) => error!("rustsekiro: travel to {map}: {e}"),
    }
}

fn open_world() -> bool {
    true
}

fn blocks() -> Vec<String> {
    BLOCKS.get().cloned().unwrap_or_default()
}

/// The play world's collision behind the combat's interface, answered as sekiro-rs's own Wolf
/// is (his body takes the world as its `Ground`, play.rs): the floor under the body's footprint,
/// the material with the deep-water rule, the camera's own collision, the ceilings. The second
/// field is when Wolf last stood on a rideable floor (the riding state, SpEffect 4100); the
/// third the same world as the enemies see it (play.rs gives them `EnemyGround`: enemy-only
/// walls and floors, hit filters 11, 16 and 19); the fourth the ground the feet are cast on, the
/// detailed `h` floors around Wolf (foot_ik.rs `FootGround`, built on a thread; the play
/// collision until it is ready).
struct World(Arc<sekiro_sim::collision::CollisionWorld>, std::sync::Mutex<Option<std::time::Instant>>, sekiro_sim::collision::EnemyGround, Option<crate::foot_ik::FootGround>);

impl World {
    fn new(world: Arc<sekiro_sim::collision::CollisionWorld>, feet: Option<crate::foot_ik::FootGround>) -> Self {
        Self(world.clone(), std::sync::Mutex::new(None), sekiro_sim::collision::EnemyGround(world), feet)
    }
}

impl rustsekiro::WorldCollision for World {
    /// The floor under the body's footprint (sim collision.rs View: five points 8 cm apart, so a
    /// seam between parts or a crack does not drop it); no triangle index (the combat reads the
    /// material separately).
    fn floor_hit(&self, x: f32, z: f32, top: f32) -> Option<(f32, u32)> {
        sekiro_sim::body::Ground::floor(&self.0, x, z, top).map(|h| (h, 0))
    }
    fn floor_material(&self, x: f32, z: f32, top: f32) -> Option<u32> {
        sekiro_sim::body::Ground::material(&self.0, x, z, top)
    }
    fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        self.0.raycast(origin, dir, max)
    }
    fn camera_raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        self.0.camera_raycast(origin, dir, max)
    }
    fn ceiling(&self, feet: [f32; 3], rise: f32) -> Option<f32> {
        sekiro_sim::body::Ground::ceiling(&self.0, feet, rise)
    }
    fn foot_floor(&self, x: f32, z: f32, top: f32) -> Option<f32> {
        let feet = self.3.as_ref().and_then(|g| {
            g.follow(x, z);
            g.height(x, z, top)
        });
        feet.or_else(|| sekiro_sim::body::Ground::floor(&self.0, x, z, top))
    }
    fn enemy_floor(&self, x: f32, z: f32, top: f32) -> Option<f32> {
        sekiro_sim::body::Ground::floor(&self.2, x, z, top)
    }
    /// The `h` ground kept around Wolf ([`Self::foot_floor`] moves it; an enemy's feet do not),
    /// else the enemy floor.
    fn enemy_foot_floor(&self, x: f32, z: f32, top: f32) -> Option<f32> {
        self.3.as_ref().and_then(|g| g.height(x, z, top)).or_else(|| self.enemy_floor(x, z, top))
    }
    fn enemy_slide(&self, feet: [f32; 3], delta: [f32; 2]) -> [f32; 2] {
        sekiro_sim::body::Ground::slide(&self.2, feet, delta)
    }
    fn slide(&self, feet: [f32; 3], delta: [f32; 2]) -> [f32; 2] {
        self.0.slide(feet, delta)
    }
    fn triangle_count(&self) -> usize {
        self.0.triangle_count()
    }
    fn water_surface(&self, x: f32, z: f32, top: f32) -> Option<(f32, u32)> {
        sekiro_sim::body::Ground::water_surface(&self.0, x, z, top).map(|w| (w.height, w.material))
    }
    /// The fall-prevention walls Wolf's script asks for (act 3016 bits: 1 movement, 2 ground
    /// actions; sim player.rs / hit_filter.rs `fall_assist_walls`), with their riding twins on a
    /// rideable floor (the Outskirts trees).
    fn slide_protected(&self, feet: [f32; 3], delta: [f32; 2], protect: u8) -> [f32; 2] {
        use sekiro_sim::hit_filter as hf;
        let mut walls = 0;
        if protect & 1 != 0 {
            walls |= hf::FALL_ASSIST_MOVE;
        }
        if protect & 2 != 0 {
            walls |= hf::FALL_ASSIST_ACTION;
        }
        if walls == 0 {
            return self.0.slide(feet, delta);
        }
        // The riding state outlasts the rideable floor by SpEffect 4100's endurance (0.06 s), so
        // a gap in a trunk's surface does not drop the ride walls (sim body.rs `riding`).
        let now = std::time::Instant::now();
        let g: &dyn sekiro_sim::body::Ground = &self.0;
        let on = g.floor_with_material(feet[0], feet[2], feet[1] + 0.1).filter(|(h, _)| feet[1] - h < g.max_step()).is_some_and(|(_, m)| hf::rideable(m));
        let riding = {
            let mut last = self.1.lock().unwrap_or_else(|e| e.into_inner());
            if on {
                *last = Some(now);
            }
            last.is_some_and(|t| now.duration_since(t).as_secs_f32() <= hf::SP_EFFECT_4100_ENDURANCE)
        };
        sekiro_sim::body::Ground::slide_assisted(&self.0, feet, delta, hf::riding_walls(walls, riding))
    }
    fn fall_return_wall(&self, from: Vec3, to: Vec3) -> bool {
        self.0.trigger_contact(from, to).is_some_and(|c| c.hit_filter == sekiro_sim::hit_filter::FALL_RETURN_FILTER)
    }
}

/// Before `DefaultPlugins`: the combat's texture source.
fn before(app: &mut App) {
    rustsekiro::register_assets(app);
}

/// After the map's plugins: the collision as the combat's terrain, the fight's place, the combat
/// with its camera and debug menu.
fn after(app: &mut App) {
    let Some((map, start)) = START.get().cloned() else { return };
    let cache = sekiro_sim::cache::root();
    let t0 = std::time::Instant::now();
    let world = (|| -> Result<_> {
        let layouts = crate::map::world::play_blocks(&cache, &map)?;
        let stories = crate::map::story::load_blocks(&cache, &layouts)?;
        let blocks: Vec<String> = layouts.keys().cloned().collect();
        let world = Arc::new(crate::map::world::collision_world(&cache, &layouts, &stories)?);
        let feet = crate::foot_ik::FootGround::load(&cache, &layouts, &stories, world.clone());
        // An idol's warp point: the player part `entity - 980` (play/idol.rs WARP_POINT).
        let warp = match start {
            Start::Idol(e) => layouts
                .values()
                .flat_map(|l| l["players"].as_array().into_iter().flatten())
                .find(|p| p["entity_id"].as_i64() == Some(e as i64 - 980))
                .and_then(|p| Some((crate::map::pose(p)?.translation, p["yaw_degrees"].as_f64().unwrap_or(0.0) as f32)))
                .map(|(t, yaw)| (t.with_y(world.floor(t.x, t.z, t.y + 1.0).unwrap_or(t.y)), yaw)),
            Start::Camera(_) => None,
        };
        let _ = BLOCKS.set(blocks.clone());
        let _ = IDOLS.set(idols(&layouts, &world));
        // The game's navmesh for the enemies' paths and mesh queries (rustsekiro::navmesh).
        let t1 = std::time::Instant::now();
        let nav = rustsekiro::navmesh::load(&cache, &layouts);
        info!("rustsekiro: navmesh, {} polygons, {:.1} s", nav.polygon_count(), t1.elapsed().as_secs_f32());
        rustsekiro::navmesh::install(Some(nav));
        Ok((world, feet, blocks, warp))
    })();
    // Wolf's start: the idol's warp point, else the camera's spot (its floor below).
    let (mut at, mut warp, mut collision) = (None, None, None);
    match world {
        Ok((w, feet, blocks, i)) => {
            edges(app, &blocks);
            info!("rustsekiro: play-world collision, {} triangles, {:.1} s", w.triangle_count(), t0.elapsed().as_secs_f32());
            if let Start::Camera([x, y, z, _]) = start {
                at = w.floor_hit(x, z, y).map(|(h, _)| Vec3::new(x, h, z));
            }
            warp = i;
            let w = World::new(w, Some(feet));
            collision = Some(w.0.clone());
            rustsekiro::Terrain::from_world(Arc::new(w)).install();
        }
        Err(e) => warn!("rustsekiro: no collision ({e}); a flat floor"),
    }
    let (x, y, z, yaw) = match start {
        Start::Camera([x, y, z, yaw]) => {
            let p = at.unwrap_or(Vec3::new(x, y - 2.0, z));
            (p.x, p.y, p.z, yaw)
        }
        Start::Idol(e) => {
            let Some((p, yaw)) = warp else {
                error!("rustsekiro: --at-idol {e}: no such idol on {map}");
                return;
            };
            info!("rustsekiro: Wolf starts at idol {e}, {p}");
            rested(e as i64);
            (p.x, p.y, p.z, yaw)
        }
    };
    let floor = y;
    // The combat's arena space: Wolf at (0, 4) facing -Z, the enemy at (0, -4). Turned to the
    // camera's yaw (0 faces -Z, as here) and moved so Wolf stands under the camera.
    let yaw = yaw.to_radians();
    let rot = Quat::from_rotation_y(yaw);
    let offset = Vec3::new(x, floor, z) - rot * Vec3::new(0.0, 0.0, 4.0);
    rustsekiro::set_frame(rustsekiro::Frame { rot, yaw, offset });
    app.add_plugins((rustsekiro::CombatCorePlugin, rustsekiro::CameraPlugin, rustsekiro::HudPlugin, rustsekiro::MenuPlugins, rustsekiro::DebugMenuPlugin, rustsekiro::SoundPlugins))
        // The combat's sound: the map's area music and indoor regions (sekiro-rs's absolute space).
        .insert_resource(rustsekiro::SoundPlace { source_map: map.clone(), sound_space: 0, space: rustsekiro::MapSpace::Absolute })
        .init_resource::<crate::map_fx::FoundIdols>()
        .add_systems(Update, (mark_camera, light_idols));
    wire(app, &cache, &map, collision);
}

/// sekiro-rs's grapple for the combat's Wolf: its hook search (`sekiro_sim::wire::Wire::search`,
/// the map's grapple.json) picks the hook the combat fires at (`rustsekiro::WireAim`, the host
/// aiming), and its wire HUD marks it (play/grapple_hud.rs). The shot and the flight are the
/// combat's own (its HKS states and homing arcs).
#[derive(Resource)]
struct HisWire {
    wire: sekiro_sim::wire::Wire,
    body: sekiro_sim::body::Body,
}

/// `collision`: the play world the hooks are aimed through (placed objects block the wire).
fn wire(app: &mut App, cache: &std::path::Path, map: &str, collision: Option<Arc<sekiro_sim::collision::CollisionWorld>>) {
    let loaded = collision
        .ok_or_else(|| anyhow::anyhow!("no collision"))
        .and_then(|world| crate::map::map_dir(cache, map).and_then(|dir| sekiro_sim::wire::Wire::load(cache, &dir, world).map_err(anyhow::Error::msg)));
    match loaded {
        Ok(wire) => {
            info!("rustsekiro: sekiro-rs's grapple, {} hooks", wire.points.len());
            app.insert_resource(HisWire { wire, body: sekiro_sim::body::Body::default() });
        }
        Err(e) => warn!("rustsekiro: no sekiro-rs grapple ({e:#}); the combat's own"),
    }
    let cache = cache.to_path_buf();
    app.add_plugins(crate::play::grapple_hud::plugin)
        .add_systems(
            Startup,
            move |mut commands: Commands,
                  mut images: ResMut<Assets<Image>>,
                  mut materials: ResMut<Assets<crate::play::grapple_hud::WireFill>>| {
                crate::play::grapple_hud::spawn(&mut commands, &cache, &mut images, &mut materials);
            },
        )
        .add_systems(PreUpdate, search_wire)
        .add_systems(Update, crate::play::grapple_hud::marker);
}

/// The hook search from Wolf's feet through the camera's look; its best hook goes to the
/// combat's aim (in reach: it fires there) and to the marker. Held during a wire move.
#[allow(clippy::type_complexity)]
fn search_wire(
    his: Option<ResMut<HisWire>>,
    wolf: Query<(&Transform, &rustsekiro::Player)>,
    camera: Query<&GlobalTransform, With<rustsekiro::HostCamera>>,
    lock: Option<Res<rustsekiro::LockOn>>,
    mut aim: ResMut<rustsekiro::WireAim>,
    mut preview: ResMut<crate::play::grapple_hud::Preview>,
) {
    let Some(mut his) = his else { return };
    let (Ok((tf, player)), Ok(eye)) = (wolf.single(), camera.single()) else { return };
    aim.host = true;
    if player.in_wire() {
        preview.0 = None;
        return;
    }
    let his = &mut *his;
    let centre = tf.translation;
    his.body.position = (centre - Vec3::Y * rustsekiro::CAPSULE_HALF_HEIGHT).to_array();
    // The search's orbit camera: yaw turns -Z, pitch is down positive (sim wire.rs WireCamera).
    let f = eye.forward();
    let (yaw, pitch) = ((-f.x).atan2(-f.z), -f.y.clamp(-1.0, 1.0).asin());
    let locked = lock.is_some_and(|l| l.target.is_some());
    his.wire.search(&his.body, yaw, pitch, locked);
    use sekiro_sim::wire::MarkerState;
    let shown = his.wire.candidate.as_ref().or(his.wire.preview.as_ref()).filter(|t| t.state != MarkerState::Hidden);
    aim.enemy = None;
    aim.target = shown.map(|t| {
        let hook = Vec3::from_array(t.point.to_array());
        rustsekiro::GrappleTarget {
            index: t.index,
            point: rustsekiro::GrapplePoint {
                hook,
                land: Vec3::from_array(t.arrival.to_array()),
                fwd: (hook - centre).with_y(0.0).normalize_or(Vec3::NEG_Z),
                variation: his.wire.points.get(t.index).map_or(0, |p| p.variation as i64),
            },
            dist: centre.distance(hook),
            in_action_range: t.state == MarkerState::Ready,
            ride: t.rideable,
        }
    });
    preview.0 = his.wire.preview.clone();
}

/// The idols Wolf has knelt at (RustSekiro's idol menu, `IdolHooks::rested`), by entity; the
/// run's own start idol is lit already.
static RESTED: std::sync::Mutex<std::collections::BTreeSet<i32>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

fn rested(entity: i64) {
    if let Ok(mut r) = RESTED.lock() {
        r.insert(entity as i32);
    }
}

/// The map's effects light the rested idols (map_fx.rs: the flame over the idol's hands in
/// place of the unlit embers), as play/idol.rs `found_idols` does from the sim.
fn light_idols(mut found: ResMut<crate::map_fx::FoundIdols>) {
    let Ok(r) = RESTED.lock() else { return };
    for &e in r.iter() {
        if found.0.insert(e) {
            info!("idol {e}: lit, its idle effect is the lit one now");
        }
    }
}

/// The map's view camera is the one RustSekiro's camera takes over (it follows Wolf, and the wire
/// HUD projects its marker through it); the map then follows Wolf, not the camera: its streaming
/// and draw groups take the collision part he stands on (as sekiro-rs's play does), so the camera
/// swinging over a ravine or through a wall never retires the map under him. The focus is his
/// feet (his play's body position), not the capsule centre Wolf's Transform holds: the standing
/// test (map_world.rs `stand`) takes the part 0.5 m below to 1 m above them, and from the centre
/// any step down lost the part under him.
#[derive(Component)]
struct WolfFeet;

#[allow(clippy::type_complexity)]
fn mark_camera(
    mut commands: Commands,
    cams: Query<Entity, (With<crate::map::MapFocus>, With<Camera3d>)>,
    wolf: Query<(&Transform, &rustsekiro::Actor), (With<rustsekiro::Player>, Without<WolfFeet>)>,
    mut feet: Query<(&mut Transform, &mut crate::map::PlayerFocus), With<WolfFeet>>,
) {
    for e in &cams {
        commands.entity(e).remove::<crate::map::MapFocus>().insert((rustsekiro::HostCamera, crate::play::grapple_hud::HudCamera));
    }
    let Ok((tf, actor)) = wolf.single() else { return };
    let (at, grounded) = (tf.translation - Vec3::Y * rustsekiro::CAPSULE_HALF_HEIGHT, !actor.airborne);
    match feet.single_mut() {
        Ok((mut t, mut f)) => {
            if t.translation != at {
                t.translation = at;
            }
            if f.grounded != grounded {
                f.grounded = grounded;
            }
        }
        Err(_) => {
            commands.spawn((WolfFeet, crate::map::MapFocus, crate::map::PlayerFocus { grounded }, Transform::from_translation(at)));
        }
    }
}

/// The play world's wire points and hang / hug edges (wall hug, ledge grab), from RustSekiro's
/// `extracted/map_edges_<map>.json` per map block (sekiro-extract edges), already absolute.
fn edges(app: &mut App, blocks: &[String]) {
    use rustsekiro::{GrapplePoint, GrapplePoints, LedgeEdge, LedgeEdges};
    // A json [x, y, z] as a Bevy point (X mirrored).
    let v = |p: &serde_json::Value, k: &str| -> Option<Vec3> {
        let a = p[k].as_array()?;
        Some(Vec3::new(-(a.first()?.as_f64()? as f32), a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
    };
    let (mut grapple, mut ledges) = (Vec::new(), Vec::new());
    for map in blocks {
        let path = rustsekiro::extracted().join(format!("map_edges_{map}.json"));
        let Some(json) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
            warn!("rustsekiro: no {}, its wire points and ledges are missing", path.display());
            continue;
        };
        for p in json["grapple"].as_array().into_iter().flatten() {
            if let (Some(hook), Some(land), Some(fwd)) = (v(p, "hook"), v(p, "land"), v(p, "fwd")) {
                grapple.push(GrapplePoint { hook, land, fwd, variation: p["var"].as_i64().unwrap_or(0) });
            }
        }
        for p in json["ledges"].as_array().into_iter().flatten() {
            if let (Some(a), Some(b), Some(c), Some(kind)) = (v(p, "a"), v(p, "b"), v(p, "c"), p["kind"].as_u64()) {
                ledges.push(LedgeEdge { a, b, c, kind: kind as u8, variation: p["var"].as_i64().unwrap_or(0) });
            }
        }
    }
    info!("rustsekiro: {} wire points, {} hang / hug edges", grapple.len(), ledges.len());
    app.insert_resource(GrapplePoints(grapple)).insert_resource(LedgeEdges(ledges));
}
