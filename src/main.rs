mod lm_render;
mod lm_bitmap;

use std::process::ExitCode;
use std::str::FromStr;
use ringhopper::definitions::{Scenario, ScenarioSpawnType, ScenarioStructureBSP, Bitmap, ScenarioSceneryPalette, ScenarioScenery, ScenarioObjectPlacement};
use ringhopper::primitives::primitive::{Angle, Euler3D, Index, TagGroup, TagPath, TagReference, Vector3D};
use ringhopper::error::Error as RinghopperError;
use ringhopper::tag::scenario_structure_bsp::get_uncompressed_vertices_for_bsp_material;
use ringhopper::tag::tree::{TagTree, VirtualTagsDirectory};
use clap::{Arg, ArgAction, ArgMatches, Command};
use clap::builder::{Styles};
use clap::{builder::styling};
use hex_color::HexColor;
use ringhopper::primitives::tag::PrimaryTagStructDyn;
use crate::lm_bitmap::{create_lm_bitmap, get_lm_page, Dimensions, LmPage};
use crate::lm_render::{BlendMode, LmRenderer, Vert};

struct SpawnInfo {
    position: Vector3D,
    facing: Angle,
}

fn main() -> ExitCode {
    let result = run_with_args(Command::new("spawn-camp")
        .about("Add spawn markers and randoms information to Halo CE multiplayer levels.")
        .version("0.1.0")
        .styles(Styles::styled()
            .header(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
            .usage(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
            .literal(styling::AnsiColor::Blue.on_default() | styling::Effects::BOLD)
            .placeholder(styling::AnsiColor::Cyan.on_default())
        )
        .arg(Arg::new("scenario-tag-path")
            .value_name("scenario-tag-path")
            .required(true)
            .help("Tag path to your scenario, for example: levels\\test\\chillout\\chillout")
        )
        .arg(Arg::new("reset")
            .long("reset")
            .short('r')
            .help("If provided, removes spawn markers from the scenario and scenery palette, and resets the BSP's lightmap reference to its previous (same named) bitmap.")
            .action(ArgAction::SetTrue)
        )
        .arg(Arg::new("tags")
            .value_name("path")
            .long("tags")
            .short('t')
            .help("Path to the base tags directory.")
            .default_value("tags")
        )
        .arg(Arg::new("marker-tag-path")
            .value_name("tag-path")
            .long("marker")
            .short('m')
            .help("Tag path for the spawn marker scenery.")
            .default_value("scenery\\spawn_marker_nhe\\spawn_marker_nhe")
        )
        .arg(Arg::new("lm-scale")
            .value_name("num")
            .long("scale")
            .short('s')
            .help("Scale for the randoms lightmap compared to Tool's lightmap. Higher scale results in sharper randoms, but increases the tag size.")
            .default_value("4")
            .value_parser(["1", "2", "4", "8", "16"])
        )
        .arg(Arg::new("randoms-color")
            .value_name("hex-code")
            .long("color")
            .short('c')
            .help("Color to render randoms in the lightmap. Supports RGB(A) hex codes like: #FF00FF, #0FF, #DDA0DD80 (alpha controls opacity).")
            .default_value("#FF000080")
        )
        .arg(Arg::new("blend")
            .value_name("mode")
            .long("blend")
            .short('b')
            .help("Color blend mode for the randoms overlay over the original lightmap.")
            .default_value("multiply")
            .value_parser(["normal", "multiply"])
        )
        .arg(Arg::new("walkable")
            .long("walkable")
            .short('w')
            .help("If provided, only walkable surfaces up to 45 degrees steepness will be shaded with the randoms color.")
            .action(ArgAction::SetTrue)
        )
        .get_matches()
    );

    match result {
        Ok(message) => {
            println!("{}", message);
            ExitCode::SUCCESS
        },
        Err(message) => {
            eprintln!("ERROR: {}", message);
            ExitCode::FAILURE
        },
    }
}

fn run_with_args(matches: ArgMatches) -> Result<String, String> {
    let scenario_tag_path = parse_tag_path(matches.get_one::<String>("scenario-tag-path").unwrap(), TagGroup::Scenario)?;
    let reset = matches.get_flag("reset");
    let tags_dir = matches.get_one::<String>("tags").unwrap();
    let marker_tag_path = parse_tag_path(matches.get_one::<String>("marker-tag-path").unwrap(), TagGroup::Scenery)?;
    let lm_scale = u16::from_str(matches.get_one::<String>("lm-scale").unwrap()).unwrap();
    let randoms_color = parse_hex_code(matches.get_one::<String>("randoms-color").unwrap())?;
    let blend_mode = parse_blend_mode(matches.get_one::<String>("blend").unwrap())?;
    let walkable_only = matches.get_flag("walkable");

    let mut tags = VirtualTagsDirectory::new(&[tags_dir], None).map_err(display_ringhopper_err)?;

    if reset {
        run_reset(&mut tags, &scenario_tag_path, &marker_tag_path)
    } else {
        run_spawns(&mut tags, &scenario_tag_path, lm_scale, randoms_color, blend_mode, walkable_only, &marker_tag_path)
    }
}

fn run_reset(tags: &mut VirtualTagsDirectory, scenario_tag_path: &TagPath, marker_tag_path: &TagPath) -> Result<String, String> {
    let mut scenario_tag = tags.open_tag_copy(&scenario_tag_path).map_err(display_ringhopper_err)?;
    let scenario = scenario_tag.get_mut::<Scenario>().unwrap();

    if let Some(bsp_tag_path) = scenario.structure_bsps.items.get(0).and_then(|scnr_bsp| scnr_bsp.structure_bsp.path()) {
        let original_lm_tag_path = get_original_lm_tag_path(&bsp_tag_path);
        println!("Resetting BSP lightmap reference to {}", original_lm_tag_path);
        let mut bsp_tag = tags.open_tag_copy(bsp_tag_path).map_err(display_ringhopper_err)?;
        let bsp = bsp_tag.get_mut::<ScenarioStructureBSP>().unwrap();
        bsp.lightmaps_bitmap = TagReference::Set(original_lm_tag_path);
        write_tag(tags, bsp_tag_path, bsp)?;
    }

    if let Some(marker_palette_index) = get_marker_palette(scenario, marker_tag_path) {
        println!("Removing marker palette entry and scenery placements");
        remove_all_markers(scenario, marker_palette_index);
        remove_marker_palette(scenario, marker_palette_index);
        write_tag(tags, scenario_tag_path, scenario)?;
    }

    Ok("Scenario reset successfully".into())
}

fn run_spawns(tags: &mut VirtualTagsDirectory, scenario_tag_path: &TagPath, lm_scale: u16, randoms_color: HexColor, blend_mode: BlendMode, walkable_only: bool, marker_tag_path: &TagPath) -> Result<String, String> {
    let mut scenario_tag = tags.open_tag_copy(&scenario_tag_path).map_err(display_ringhopper_err)?;
    let scenario = scenario_tag.get_mut::<Scenario>().unwrap();

    let slayer_spawns = get_slayer_spawns(scenario);
    generate_randoms(tags, &slayer_spawns, scenario, lm_scale, randoms_color, blend_mode, walkable_only)?;
    place_spawn_markers(tags, &slayer_spawns, scenario, marker_tag_path)?;
    write_tag(tags, scenario_tag_path, scenario)?;

    Ok("Spawns added successfully".into())
}

fn place_spawn_markers(tags: &mut VirtualTagsDirectory, slayer_spawns: &[SpawnInfo], scenario: &mut Scenario, marker_tag_path: &TagPath) -> Result<(), String> {
    tags.open_tag_copy(marker_tag_path).map_err(|_|
        format!("No marker scenery tag exists at path {}. You can get it from https://github.com/khstarr/h1-spawn-tools", marker_tag_path)
    )?;

    let marker_palette_index = match get_marker_palette(scenario, marker_tag_path) {
        Some(index) => {
            println!("Removing existing markers");
            remove_all_markers(scenario, index);
            index
        },
        None => {
            println!("Adding scenery palette entry {}", marker_tag_path);
            scenario.scenery_palette.items.push(ScenarioSceneryPalette {
                name: TagReference::Set(marker_tag_path.clone())
            });
            Some(scenario.scenery_palette.items.len() as u16 - 1)
        }
    };

    println!("Placing {} spawn markers", slayer_spawns.len());
    scenario.scenery.items.extend(slayer_spawns.iter().map(|spawn| {
        ScenarioScenery {
            _type: marker_palette_index,
            name: None,
            placement: ScenarioObjectPlacement {
                position: spawn.position,
                rotation: Euler3D {
                    yaw: spawn.facing,
                    pitch: Angle::default(),
                    roll: Angle::default(),
                },
                ..ScenarioObjectPlacement::default()
            },
            ..ScenarioScenery::default()
        }
    }));

    Ok(())
}

fn get_marker_palette(scenario: &Scenario, marker_tag_path: &TagPath) -> Option<Index> {
    scenario.scenery_palette.items.iter()
        .position(|palette_entry| palette_entry.name.path().map(|tag_path| tag_path.eq(marker_tag_path)).unwrap_or(false))
        .map(|i| Some(i as u16))
}

fn remove_all_markers(scenario: &mut Scenario, marker_palette_index: Index) {
    scenario.scenery.items = scenario.scenery.items.iter()
        .filter(|scenery| scenery._type != marker_palette_index)
        .cloned()
        .collect();
}

fn remove_marker_palette(scenario: &mut Scenario, marker_palette_index: Index) {
    if let Some(i) = marker_palette_index {
        scenario.scenery_palette.items.remove(i as usize);

        //any scenery using a palette index greater than i needs to be reduced
        scenario.scenery.items.iter_mut().for_each(|scenery| {
            if let Some(scenery_type) = scenery._type {
                if scenery_type > i {
                    scenery._type = Some(scenery_type - 1);
                }
            }
        });
    }
}

fn generate_randoms(tags: &mut VirtualTagsDirectory, slayer_spawns: &[SpawnInfo], scenario: &Scenario, scale: u16, randoms_color: HexColor, blend_mode: BlendMode, walkable_only: bool) -> Result<(), String> {
    let bsp_tag_path = scenario.structure_bsps.items.get(0).ok_or("The scenario has no BSP")
        ?.structure_bsp.path().ok_or("The scenario's BSP tag path is empty")?;

    println!("Generating randoms for BSP {} ", bsp_tag_path);
    let renderer = LmRenderer::init(slayer_spawns, randoms_color, blend_mode, walkable_only);

    let mut bsp_tag = tags.open_tag_copy(bsp_tag_path).map_err(display_ringhopper_err)?;
    let bsp = bsp_tag.get_mut::<ScenarioStructureBSP>().unwrap();

    let original_lm_tag_path = get_original_lm_tag_path(&bsp_tag_path);
    let original_lm_tag = tags.open_tag_copy(&original_lm_tag_path).unwrap();
    let original_lm = original_lm_tag.get_ref::<Bitmap>().unwrap();

    let output_pages: Vec<LmPage> = bsp.lightmaps.items.iter().filter_map(|bsp_lightmap| {
        bsp_lightmap.bitmap.map(|lm_bitmap_index| {
            let mut verts: Vec<Vert> = Vec::new();
            let mut indices: Vec<u16> = Vec::new();

            //base the output dimensions on the original lightmap's dimensions
            let output_dimensions = original_lm.bitmap_data.items.get(lm_bitmap_index as usize).map(|prev_lm_bitmap_data| {
                Dimensions {
                    w: prev_lm_bitmap_data.width * scale,
                    h: prev_lm_bitmap_data.height * scale,
                }
            }).unwrap();

            bsp_lightmap.materials.items.iter().for_each(|material| {
                let (rendered_verts, lm_verts) = get_uncompressed_vertices_for_bsp_material(material).unwrap();
                let rendered_verts = rendered_verts.collect::<Vec<_>>();

                let offset = verts.len() as u16;
                verts.extend(lm_verts
                    .enumerate()
                    .map(|(i, v)| {
                        let rendered_vert = rendered_verts.get(i).unwrap();
                        Vert {
                            lm_uv: [
                                v.texture_coords.x as f32,
                                v.texture_coords.y as f32
                            ],
                            world_pos: [
                                rendered_vert.position.x as f32,
                                rendered_vert.position.y as f32,
                                rendered_vert.position.z as f32,
                            ],
                            world_normal: [
                                rendered_vert.normal.x as f32,
                                rendered_vert.normal.y as f32,
                                rendered_vert.normal.z as f32,
                            ]
                        }
                    })
                );

                indices.extend((material.surfaces..(material.surfaces + material.surface_count))
                    .flat_map(|surface_index| {
                        let bsp_surface = bsp.surfaces.items.get(surface_index as usize).unwrap();
                        [
                            bsp_surface.vertex0_index.unwrap() + offset,
                            bsp_surface.vertex1_index.unwrap() + offset,
                            bsp_surface.vertex2_index.unwrap() + offset
                        ]
                    })
                );
            });

            println!("Rendering lightmap {} with {} verts [{}x{}]", lm_bitmap_index, verts.len(), output_dimensions.w, output_dimensions.h);
            let original_lm_page = get_lm_page(original_lm, lm_bitmap_index).unwrap();
            renderer.render_randoms(verts, indices, output_dimensions, &original_lm_page)
        })
    }).collect();

    println!("Assembling LM bitmap");
    let output_lm_tag_path = get_output_lm_tag_path(bsp_tag_path);
    let output_lm = create_lm_bitmap(&output_pages);
    write_tag(tags, &output_lm_tag_path, &output_lm)?;

    println!("Updating BSP lightmap bitmap reference");
    bsp.lightmaps_bitmap = TagReference::Set(output_lm_tag_path);
    write_tag(tags, bsp_tag_path, bsp)?;

    Ok(())
}

fn write_tag(tags: &mut VirtualTagsDirectory, tag_path: &TagPath, tag: &dyn PrimaryTagStructDyn) -> Result<(), String> {
    println!("Writing tag {}", tag_path);
    tags.write_tag(tag_path, tag).map_err(display_ringhopper_err)?;
    Ok(())
}

fn get_slayer_spawns(scenario: &Scenario) -> Vec<SpawnInfo> {
    scenario.player_starting_locations.items.iter().filter_map(|loc| {
        if is_slayer_spawn(loc.type_0) || is_slayer_spawn(loc.type_1) || is_slayer_spawn(loc.type_2) || is_slayer_spawn(loc.type_3) {
            Some(SpawnInfo {
                position: loc.position,
                facing: loc.facing,
            })
        } else {
            None
        }
    }).collect()
}

fn is_slayer_spawn(spawn_type: ScenarioSpawnType) -> bool {
    match spawn_type {
        ScenarioSpawnType::Slayer => true,
        ScenarioSpawnType::AllGames => true,
        ScenarioSpawnType::AllExceptCtf => true,
        ScenarioSpawnType::AllExceptRaceAndCtf => true,
        _ => false,
    }
}

//tool.exe creates lightmap bitmaps with the same tag path as the BSP
fn get_original_lm_tag_path(bsp_tag_path: &TagPath) -> TagPath {
    TagPath::new(bsp_tag_path.path(), TagGroup::Bitmap).unwrap()
}

fn get_output_lm_tag_path(bsp_tag_path: &TagPath) -> TagPath {
    TagPath::new(&format!("{}_randoms", bsp_tag_path.path()), TagGroup::Bitmap).unwrap()
}

fn parse_tag_path(raw: &str, group: TagGroup) -> Result<TagPath, String> {
    TagPath::new(raw, group).map_err(display_ringhopper_err)
}

fn parse_hex_code(raw: &str) -> Result<HexColor, String> {
    let prefixed = if raw.starts_with("#") {
        raw.into()
    } else {
        format!("#{raw}")
    };
    HexColor::parse(&prefixed).map_err(|_| format!("Not a valid hex color code: {}", raw))
}

fn parse_blend_mode(raw: &str) -> Result<BlendMode, String> {
    match raw.to_ascii_lowercase().as_str() {
        "normal" => Ok(BlendMode::Normal),
        "multiply" => Ok(BlendMode::Multiply),
        _ => Err(format!("Not a valid blend mode: {}", raw)),
    }
}

fn display_ringhopper_err(err: RinghopperError) -> String {
    match err {
        RinghopperError::InvalidTagsDirectory => "Invalid tags directory".into(),
        _ => format!("Unexpected error: {}", err.as_str()),
    }
}