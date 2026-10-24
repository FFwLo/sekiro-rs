//! Loads the tunable gap values from `config.toml` and reloads them on F5.
//! Values that Sekiro's data provides are not here; see data.rs.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// All tuning values. The layout mirrors the sections in `config.toml`.
#[derive(Resource, Deserialize, Clone, Debug)]
pub struct GameConfig {
    pub camera: CameraConfig,
    pub movement: MovementConfig,
    #[serde(default)]
    pub player: PlayerConfig,
    pub input: InputConfig,
    pub posture: PostureConfig,
    pub combat: CombatConfig,
    pub enemy: EnemyConfig,
    #[serde(default)]
    pub world: WorldConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub video: VideoConfig,
}

/// The Options screen's sound sliders (menu.rs), 0-10 like the game's. `volume` is ours (the
/// game has none): the master category at volume / 10 on top of the game's own 0.7 (fmod.rs
/// MASTER_VOLUME). `music` / `se` / `voice` are the game's option bytes 4 / 5 / 6 (Bgm / Se /
/// Voice), each its FMOD categories at value / 10 (fmod.rs set_mix).
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct AudioConfig {
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default = "default_volume")]
    pub music: f32,
    #[serde(default = "default_volume")]
    pub se: f32,
    #[serde(default = "default_volume")]
    pub voice: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig { volume: default_volume(), music: default_volume(), se: default_volume(), voice: default_volume() }
    }
}

/// The game's slider default (GameDataMan options bytes 4-6 = 10, FUN_1407bc470).
fn default_volume() -> f32 {
    10.0
}

/// The window (menu.rs apply_video): borderless fullscreen or a window, vsync on / off.
#[derive(Deserialize, Clone, Debug)]
pub struct VideoConfig {
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default = "default_true")]
    pub vsync: bool,
    /// Window size in pixels (menu.rs apply_video); None = the default 1024x560.
    #[serde(default)]
    pub resolution: Option<[u32; 2]>,
    /// Sun shadow map size in pixels (map.rs, the frame rate's main knob); None = Bevy's 2048.
    /// `SHINOBI_MAP_SHADOW_RES` overrides it for one run.
    #[serde(default)]
    pub shadow_res: Option<usize>,
    /// Cloth physics (cloth.rs, the Options menu): "on" within 15 m of the camera, "near"
    /// within 6 m, "off" (every coat carried stiffly in its bind pose).
    /// `SHINOBI_CLOTH_RANGE` overrides it for one run.
    #[serde(default)]
    pub cloth: ClothSetting,
    /// The HUD and menus on a centred 16:9 stage ("16:9", ui_stage.rs) or across the whole
    /// window ("stretch"); only matters on a window wider than 16:9.
    #[serde(default)]
    pub hud_aspect: HudAspect,
}

#[derive(serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HudAspect {
    #[default]
    #[serde(rename = "16:9")]
    Stage,
    #[serde(rename = "stretch")]
    Stretch,
}

impl HudAspect {
    /// The config.toml spelling.
    pub fn name(self) -> &'static str {
        match self {
            HudAspect::Stage => "16:9",
            HudAspect::Stretch => "stretch",
        }
    }
}

#[derive(serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClothSetting {
    #[default]
    On,
    Near,
    Off,
}

impl ClothSetting {
    /// The simulation range in metres from the camera; negative = none simulated.
    pub fn range(self) -> f32 {
        match self {
            ClothSetting::On => 15.0,
            ClothSetting::Near => 6.0,
            ClothSetting::Off => -1.0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ClothSetting::On => "on",
            ClothSetting::Near => "near",
            ClothSetting::Off => "off",
        }
    }
}

impl Default for VideoConfig {
    fn default() -> Self {
        VideoConfig { fullscreen: false, vsync: true, resolution: None, shadow_res: None, cloth: ClothSetting::On, hud_aspect: HudAspect::Stage }
    }
}

/// Sets `key = value` in `[table]` of config.toml's text, keeping every other line and comment
/// (adds the key under the table, or the table at the end, when missing).
pub fn set_key(text: &str, table: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let (mut inside, mut seen, mut done) = (false, false, false);
    let header = format!("[{table}]");
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            if inside && !done {
                // Before the next table (and the blank / comment lines that lead into it).
                let at = out.iter().rposition(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')).map_or(out.len(), |i| i + 1);
                out.insert(at, format!("{key} = {value}"));
                done = true;
            }
            inside = t.starts_with(&header);
            seen |= inside;
        }
        if inside && !done && t.starts_with(key) && t[key.len()..].trim_start().starts_with('=') {
            // The trailing comment with the spaces before it.
            let comment = t.find('#').map(|i| t[t[..i].trim_end().len()..].to_string()).unwrap_or_default();
            out.push(format!("{key} = {value}{comment}"));
            done = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if !seen {
            out.push(String::new());
            out.push(header);
        }
        out.push(format!("{key} = {value}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Writes these `(table, key, value)` into config.toml (set_key each).
pub fn save_keys(keys: &[(&str, &str, String)]) -> Result<(), String> {
    let path = GameConfig::path();
    let mut text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    for (table, key, value) in keys {
        text = set_key(&text, table, key, value);
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The arena: the game's own map (`sekiro-extract map` output) or the flat test floor.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct WorldConfig {
    /// Map id of `extracted/map_<id>.bin` (e.g. "m11_01_00_00"); unset = flat arena.
    #[serde(default)]
    pub map: Option<String>,
    /// Time of day for the draw params (hours). A boss fight takes its own draw param variant's
    /// hour (map.rs). The game has no clock: common.emevd event 100 picks the castle /
    /// outskirts light from the cumulative story flags 9800-9803 (none 6 h morning, 9800 12 h,
    /// +9801 18 h, +9802 22 h, all 2 h dawn), and event 820 raises 9800 / 9801 after one / three
    /// of the time bosses (9303-9306) fall (sekiro-rs 9ee18b0 crates/sim/src/story.rs, checked
    /// there against the installed event script). Sv1 has no story flags: morning.
    #[serde(default = "default_hour")]
    pub hour: f32,
}

fn default_hour() -> f32 {
    6.0
}

#[derive(Deserialize, Clone, Debug)]
pub struct CameraConfig {
    pub mouse_sensitivity: f32,
    /// How far the free camera swings round after Wolf's sideways move, 0-1 (1 = the
    /// CameraParam fulcrum's full swing; camera.rs).
    #[serde(default = "default_follow_swing")]
    pub follow_swing: f32,
}

fn default_follow_swing() -> f32 {
    0.5
}

#[derive(Deserialize, Clone, Debug)]
pub struct MovementConfig {
    pub turn_speed: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PlayerConfig {
    /// Healing Gourd charges (goods 3000 maxNum 10; one more per Gourd Seed in the game).
    pub gourd_charges: u32,
    /// Spirit Emblems carried (prosthetic ammunition).
    #[serde(default = "default_emblems")]
    pub spirit_emblems: u32,
    /// Testing cheat: the Spirit Emblems never run out (also the F1 menu's toggle).
    #[serde(default)]
    pub infinite_emblems: bool,
    /// Resurrection nodes (the user, 2026-10-18: "2 ressurection nodes just like in base game, one
    /// base and one extra").
    #[serde(default = "default_resurrections")]
    pub resurrections: u32,
    /// Equipped combat art = its virtual weapon (EquipParamWeapon): 5100 Whirlwind Slash,
    /// 5300 Ichimonji, 5200 / 5400-5900 the others (anim group a<spAtkcategory>).
    #[serde(default = "default_combat_art")]
    pub combat_art: i64,
    /// Equipped Shinobi ninjutsu (EquipParamGoods, goodsType 8; 0 = none): 2100 Bloodsmoke,
    /// 2110 Puppeteer, 2120 Bestowal (ninjutsu.rs).
    #[serde(default)]
    pub ninjutsu: i64,
    /// Prosthetic tool slots (EquipParamWeapon 70000-79200, up to 3 like the game; Z switches):
    /// 70000 Loaded Shuriken, 71000 Shinobi Firecracker, 72000 Flame Vent, 73000 Loaded Axe,
    /// 74000 Mist Raven, 75000 Sabimaru, 76000 Loaded Umbrella, 77000 Divine Abduction,
    /// 78000 Loaded Spear, 79000 Finger Whistle (+100 per upgrade level).
    /// Left out of config.toml: the first three `owned_tools` in their order, so Z switches
    /// between the tools the start holds (the game's new game, CharaInitParam 10000, fits none;
    /// here the start owns `owned_tools`).
    #[serde(default)]
    pub prosthetics: Vec<i64>,
    /// Learned latent skills (SkillParam ids), e.g. 280 Flowing Water (less posture damage when guarding).
    /// Empty = level-1 Wolf.
    #[serde(default)]
    pub skills: Vec<i64>,
    /// Skill points to spend at a Sculptor's Idol (Acquire Skills, src/skill_tree.rs); defeated
    /// enemies' experience adds more (src/experience.rs).
    #[serde(default)]
    pub skill_points: u32,
    /// Experience towards the next skill point.
    #[serde(default)]
    pub skill_exp: u32,
    /// Skill points earned from experience so far, spent or not (the next costs more).
    #[serde(default)]
    pub skill_level: u32,
    /// Wolf's look (Change Form at an idol, src/form.rs): 0 Wolf, 1 Shura, 2 Ashina, 3 Tengu.
    #[serde(default)]
    pub form: u32,
    /// Goods held besides the gourd and the emblems: [EquipParamGoods id, count] (src/items.rs).
    #[serde(default)]
    pub items: Vec<(i64, u32)>,
    /// The five quick-item slots (goods ids, 0 empty); R uses the one in hand, C moves on.
    #[serde(default = "default_quick_items")]
    pub quick_items: Vec<i64>,
    /// Base prosthetic tools made (EquipParamWeapon 7X000; the Sculptor's Workshop,
    /// src/workshop.rs). Default: all ten.
    #[serde(default = "default_owned_tools")]
    pub owned_tools: Vec<i64>,
    /// Prosthetic tool upgrades bought (SkillParam 700+, workshop.rs).
    #[serde(default)]
    pub upgrades: Vec<i64>,
    /// Every move the game locks behind a skill (`combat::action_unlocked`: mid-air deflect / tools
    /// / arts, the sprint slide, the Prosthetic Arts follow-ups) is open, as for a Wolf with the
    /// whole tree. false = only those the `skills` list unlocks.
    #[serde(default = "default_true")]
    pub unlock_all_moves: bool,
    /// Sen carried: the Sen Throw (70400) throws by it (common.emevd sets 127060-127063,
    /// stateInfo 999-996: 0 / 1-249 / 250-499 / 500+). Default 500 (the full throw).
    #[serde(default = "default_sen")]
    pub sen: u32,
    /// Attack Power (1 at the start; each Memory offered adds 1, up to 99): Wolf wears SpEffect
    /// 160000 + it, "Player Growth_Core Level N" (combat::growth_sp).
    #[serde(default = "default_attack_power")]
    pub attack_power: u32,
}

fn default_attack_power() -> u32 {
    1
}

fn default_owned_tools() -> Vec<i64> {
    (70000..80000).step_by(1000).collect()
}

fn default_quick_items() -> Vec<i64> {
    vec![3000, 0, 0, 0, 0]
}

fn default_sen() -> u32 {
    500
}

fn default_true() -> bool {
    true
}

fn default_combat_art() -> i64 {
    5100
}

/// The tool slots when config.toml names none: the first three tools owned.
fn starting_tools(owned: &[i64]) -> Vec<i64> {
    owned.iter().copied().take(3).collect()
}

fn default_enemy_chr() -> String {
    "c1020".to_string()
}

fn default_resurrections() -> u32 {
    2
}

fn default_emblems() -> u32 {
    15
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self { gourd_charges: 3, spirit_emblems: default_emblems(), infinite_emblems: false, resurrections: default_resurrections(), combat_art: default_combat_art(), ninjutsu: 0, prosthetics: starting_tools(&default_owned_tools()), skills: Vec::new(), skill_points: 0, skill_exp: 0, skill_level: 0, form: 0, items: Vec::new(), quick_items: default_quick_items(), owned_tools: default_owned_tools(), upgrades: Vec::new(), unlock_all_moves: true, sen: default_sen(), attack_power: default_attack_power() }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct InputConfig {
    pub buffer: f32,
    /// A gamepad drives Wolf, the camera and the menus (plan C4).
    #[serde(default = "default_true")]
    pub gamepad: bool,
    /// The game's camera speed option, 1-10: the right stick turns at CameraParam
    /// rotSpeed_Min + (Max - Min) * (speed / 10)^2 (camera.rs stick_rate).
    #[serde(default = "default_camera_speed")]
    pub camera_speed: f32,
    /// Right stick / mouse up looks down.
    #[serde(default)]
    pub invert_y: bool,
}

/// Byte 0 of the game's option defaults is 5 (FUN_1407bc470, beside the volume bytes 4-6 = 10);
/// it is the camera speed: FUN_140734790 hands byte 0 / 10 to the camera (+0x270), which scales
/// the CameraParam stick speeds by it (FUN_14073c260). gap: the menu's slider range (1-10 here).
fn default_camera_speed() -> f32 {
    5.0
}

#[derive(Deserialize, Clone, Debug)]
pub struct PostureConfig {
    pub default_ratio_type: i64,
    pub regen_delay: f32,
    pub hp_scales_regen: bool,
    pub player_level: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CombatConfig {
    pub player_reach: f32,
    pub enemy_reach: f32,
    pub attack_arc: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct EnemyConfig {
    /// Which enemy to fight: "c1020" Samurai General, "c1010" Ochimusha.
    #[serde(default = "default_enemy_chr")]
    pub chr: String,
    pub attack_range: f32,
    pub cooldown_min: f32,
    pub cooldown_max: f32,
    pub broken_time: f32,
    #[serde(default = "default_doping")]
    pub area_doping: i64,
    /// NpcParam row (outfit variant / stats), e.g. c1020 10203010 no haori, 10200010 with haori.
    /// Unset: the default row of `chr`.
    #[serde(default)]
    pub npc_row: Option<i64>,
    /// More enemies fought at the same time, e.g.
    /// `group = [{ chr = "c1010", at = [3.0, -6.0] }, { chr = "c1020", npc_row = 10200010 }]`.
    /// Empty: only `chr`.
    #[serde(default)]
    pub group: Vec<GroupEnemy>,
}

/// One more enemy (config enemy.group): its chr, NpcParam row (unset: its default) and where it
/// stands (x, z; unset: in a row beside the first). `msb`: an MSB enemy's entity id on the
/// loaded map (extracted/enemies/patrols.json): it stands at its MSB spot facing its MSB way and
/// walks its patrol route (enemy.rs msb_patrol), e.g. `{ chr = "c1010", npc_row = 10102010,
/// msb = 1110236 }` on the gate map.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct GroupEnemy {
    pub chr: String,
    #[serde(default)]
    pub npc_row: Option<i64>,
    #[serde(default)]
    pub at: Option<[f32; 2]>,
    #[serde(default)]
    pub msb: Option<i64>,
}

impl GameConfig {
    /// `config.toml` sits next to `Cargo.toml`, wherever the game is started from.
    fn path() -> PathBuf {
        crate::paths::root().join("config.toml")
    }

    pub(crate) fn load() -> Result<Self, String> {
        let path = Self::path();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut config: GameConfig = toml::from_str(text).map_err(|e| format!("config.toml has a mistake: {e}"))?;
        let raw: toml::Value = toml::from_str(text).map_err(|e| format!("config.toml has a mistake: {e}"))?;
        if raw.get("player").and_then(|p| p.get("prosthetics")).is_none() {
            config.player.prosthetics = starting_tools(&config.player.owned_tools);
        }
        Ok(config)
    }
}

/// What carries over of Wolf's state between runs and F5 (save.json next to config.toml):
/// the equipment (tools, art, ninjutsu, quick items), the bag and Sen, the tools made and
/// upgrades bought, the skills learned and their points. Idol Travel starts this program again
/// (sekiro-rs's `travel`), so without it every change made in play was lost there and on F5.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub(crate) struct Kept {
    prosthetics: Vec<i64>,
    combat_art: i64,
    ninjutsu: i64,
    quick_items: Vec<i64>,
    items: Vec<(i64, u32)>,
    sen: u32,
    owned_tools: Vec<i64>,
    upgrades: Vec<i64>,
    skills: Vec<i64>,
    skill_points: u32,
    skill_exp: u32,
    skill_level: u32,
}

impl Kept {
    pub(crate) fn of(p: &PlayerConfig) -> Self {
        Kept {
            prosthetics: p.prosthetics.clone(),
            combat_art: p.combat_art,
            ninjutsu: p.ninjutsu,
            quick_items: p.quick_items.clone(),
            items: p.items.clone(),
            sen: p.sen,
            owned_tools: p.owned_tools.clone(),
            upgrades: p.upgrades.clone(),
            skills: p.skills.clone(),
            skill_points: p.skill_points,
            skill_exp: p.skill_exp,
            skill_level: p.skill_level,
        }
    }

    pub(crate) fn put(self, p: &mut PlayerConfig) {
        p.prosthetics = self.prosthetics;
        p.combat_art = self.combat_art;
        p.ninjutsu = self.ninjutsu;
        p.quick_items = self.quick_items;
        p.items = self.items;
        p.sen = self.sen;
        p.owned_tools = self.owned_tools;
        p.upgrades = self.upgrades;
        p.skills = self.skills;
        p.skill_points = self.skill_points;
        p.skill_exp = self.skill_exp;
        p.skill_level = self.skill_level;
    }
}

/// save.json: the kept state (`now`) and config.toml's values when it was made (`base`).
#[derive(Serialize, Deserialize)]
struct SaveFile {
    base: Kept,
    now: Kept,
}

/// config.toml's kept values as last loaded (the next save's `base`).
#[derive(Resource, Default)]
pub(crate) struct SaveBase(pub(crate) Kept);

fn save_path() -> PathBuf {
    crate::paths::root().join("save.json")
}

/// The kept state over config.toml's: each value the save's, unless config.toml's own changed
/// since the save was made (an edit to the file, then F5 or a new run, wins). Delete save.json
/// to start again from config.toml.
pub(crate) fn merged(file: &Kept, base: &Kept, now: &Kept) -> Kept {
    let (f, b, n) = (serde_json::to_value(file).unwrap_or_default(), serde_json::to_value(base).unwrap_or_default(), serde_json::to_value(now).unwrap_or_default());
    let Some(fields) = f.as_object() else { return file.clone() };
    let out: serde_json::Map<String, serde_json::Value> = fields.iter().map(|(k, fv)| (k.clone(), if b.get(k) != Some(fv) { fv.clone() } else { n.get(k).cloned().unwrap_or_else(|| fv.clone()) })).collect();
    serde_json::from_value(serde_json::Value::Object(out)).unwrap_or_else(|_| file.clone())
}

fn read_save(path: &std::path::Path) -> Option<SaveFile> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_save_file(path: &std::path::Path, base: &Kept, now: &Kept) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&SaveFile { base: base.clone(), now: now.clone() }).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes save.json when the kept state (or config.toml's under it, after F5) changes. Not in
/// the tests: they run on config.toml alone.
fn write_save(config: Res<GameConfig>, base: Res<SaveBase>, mut last: Local<Option<(Kept, Kept)>>) {
    if cfg!(test) || !(config.is_changed() || base.is_changed()) {
        return;
    }
    let now = Kept::of(&config.player);
    if last.as_ref().is_some_and(|(b, n)| *b == base.0 && *n == now) {
        return;
    }
    // The first look is the state as loaded: nothing to write yet.
    if last.replace((base.0.clone(), now.clone())).is_none() {
        return;
    }
    if let Err(e) = write_save_file(&save_path(), &base.0, &now) {
        warn!("save.json not written: {e}");
    }
}

pub struct ConfigPlugin;

impl Plugin for ConfigPlugin {
    fn build(&self, app: &mut App) {
        let mut config = GameConfig::load().unwrap_or_else(|e| panic!("{e}"));
        let base = Kept::of(&config.player);
        if !cfg!(test) {
            if let Some(save) = read_save(&save_path()) {
                merged(&base, &save.base, &save.now).put(&mut config.player);
                info!("save.json: Wolf's equipment, bag and skills carried over");
            }
        }
        app.insert_resource(config)
            .insert_resource(SaveBase(base))
            .add_systems(Update, reload_on_f5.run_if(resource_exists::<ButtonInput<KeyCode>>))
            .add_systems(Last, write_save);
    }
}

fn reload_on_f5(keys: Res<ButtonInput<KeyCode>>, mut config: ResMut<GameConfig>, mut base: ResMut<SaveBase>) {
    if !keys.just_pressed(KeyCode::F5) {
        return;
    }
    match GameConfig::load() {
        Ok(new_config) => {
            *config = reloaded(&config, new_config, &mut base.0);
            info!("config.toml reloaded");
        }
        // A typo in the file keeps the old values instead of crashing the game.
        Err(e) => error!("{e} (keeping the previous values)"),
    }
}

/// The file's values, but the fight in progress stays: the encounter picker (enemy.rs
/// spawn_encounter) sets enemy.chr / npc_row / group in memory only, and Combat.kinds holds that
/// encounter; taking the file's back made the next reset spawn its group into the picked fight.
/// Wolf's kept state (`Kept`: equipment, bag, skills) stays too, unless the file's own value
/// changed since the last load (`base`, updated to the file's).
pub(crate) fn reloaded(old: &GameConfig, mut new: GameConfig, base: &mut Kept) -> GameConfig {
    new.enemy.chr = old.enemy.chr.clone();
    new.enemy.npc_row = old.enemy.npc_row;
    new.enemy.group = old.enemy.group.clone();
    let file = Kept::of(&new.player);
    merged(&file, base, &Kept::of(&old.player)).put(&mut new.player);
    *base = file;
    new
}

#[cfg(test)]
mod tests {
    /// F5 keeps the picked fight (2026-10-12 bug hunt round 9).
    #[test]
    fn f5_keeps_the_picked_encounter() {
        let mut old = super::GameConfig::load().expect("config.toml must parse");
        old.enemy.chr = "c1010".into();
        old.enemy.npc_row = Some(10102010);
        old.enemy.group.clear();
        let mut file = super::GameConfig::load().unwrap();
        file.enemy.chr = "c1020".into();
        file.enemy.group.push(crate::config::GroupEnemy { chr: "c1010".into(), npc_row: Some(10102010), at: None, msb: Some(1110236) });
        let mut base = super::Kept::of(&file.player);
        let got = super::reloaded(&old, file, &mut base);
        assert_eq!((got.enemy.chr.as_str(), got.enemy.npc_row, got.enemy.group.len()), ("c1010", Some(10102010), 0));
    }

    #[test]
    fn config_toml_parses() {
        super::GameConfig::load().expect("config.toml must parse");
    }

    /// The Options writer keeps the comments, replaces in place and adds what is missing.
    #[test]
    fn set_key_keeps_the_file() {
        let text = "# top\n[input]\n# the buffer\nbuffer = 0.0  # seconds\n\n[posture]\nx = 1\n";
        let t = super::set_key(text, "input", "buffer", "0.5");
        assert!(t.contains("buffer = 0.5  # seconds") && t.contains("# the buffer") && t.contains("# top"), "{t}");
        let t = super::set_key(&t, "input", "invert_y", "true");
        assert!(t.find("invert_y = true").unwrap() < t.find("[posture]").unwrap(), "{t}");
        let t = super::set_key(&t, "video", "vsync", "false");
        assert!(t.ends_with("[video]\nvsync = false\n"), "{t}");
        let c: toml::Value = toml::from_str(&t).unwrap();
        assert_eq!(c["input"]["buffer"].as_float(), Some(0.5));
    }
}

fn default_doping() -> i64 {
    7010
}
