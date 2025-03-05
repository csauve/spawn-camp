mod render;

use std::process::ExitCode;
use std::str::FromStr;
use ringhopper::definitions::{
    Scenario, ScenarioSpawnType, ScenarioStructureBSP, Bitmap, BitmapType, BitmapFormat, BitmapUsage,
    BitmapGroupSequence, BitmapData, BitmapDataType, BitmapDataFormat, BitmapDataFlags
};
use ringhopper::primitives::primitive::{Data, Reflexive, TagGroup, TagPath, TagReference, Vector2DInt};
use ringhopper::error::Error as RinghopperError;
use ringhopper::tag::scenario_structure_bsp::get_uncompressed_vertices_for_bsp_material;
use ringhopper::tag::tree::{TagTree, VirtualTagsDirectory};
use clap::{Arg, ArgAction, ArgMatches, Command};
use clap::builder::{Styles};
use clap::{builder::styling};
use colored::{Colorize};
use hex_color::HexColor;
use crate::render::{Dimensions, LmRenderer, Vert};

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
            .default_value("2")
            .value_parser(["1", "2", "4", "8"])
        )
        .arg(Arg::new("randoms-color")
            .value_name("hex-code")
            .long("color")
            .short('c')
            .help("Color to render randoms in the lightmap. Supports RGB(A) hex codes like: #FF00FF, #0FF, #DDA0DD80 (alpha controls multiply opacity)")
            .default_value("#FF000080")
        )
        .get_matches()
    );

    match result {
        Ok(message) => {
            println!("{}", message.green());
            ExitCode::SUCCESS
        },
        Err(message) => {
            eprintln!("{}", message.red());
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

    if reset {
        run_reset(tags_dir)
    } else {
        run_spawns(tags_dir, &scenario_tag_path, lm_scale, randoms_color)
    }
}

fn run_reset(tags_dir: &str) -> Result<String, String> {
    Ok("Scenario reset successfully".into())
}

fn run_spawns(tags_dir: &str, scenario_tag_path: &TagPath, lm_scale: u16, randoms_color: HexColor) -> Result<String, String> {
    let mut tags = VirtualTagsDirectory::new(&[tags_dir], None).map_err(display_ringhopper_err)?;

    let scenario_tag = tags.open_tag_copy(&scenario_tag_path).map_err(display_ringhopper_err)?;
    let scenario = scenario_tag.get_ref::<Scenario>().unwrap();

    let bsp_tag_path = scenario.structure_bsps.items.get(0).ok_or("The scenario has no BSP")
        ?.structure_bsp.path().ok_or("The scenario's BSP tag path is empty")?;

    let slayer_spawns = get_slayer_spawns(scenario);

    generate_randoms(&mut tags, slayer_spawns, bsp_tag_path, lm_scale, randoms_color)?;


    Ok("Spawns added successfully".into())
}

fn generate_randoms(tags: &mut VirtualTagsDirectory, slayer_spawns: Vec<[f32; 3]>, bsp_tag_path: &TagPath, scale: u16, randoms_color: HexColor) -> Result<(), String> {
    println!("{}", format!("Generating randoms for BSP {} ", bsp_tag_path).bright_blue());
    let renderer = LmRenderer::init(slayer_spawns, randoms_color);

    let mut bsp_tag = tags.open_tag_copy(bsp_tag_path).map_err(display_ringhopper_err)?;
    let bsp = bsp_tag.get_mut::<ScenarioStructureBSP>().unwrap();
    let original_lm_tag_path = get_original_lm_tag_path(&bsp_tag_path);

    //base the output dimensions on the original lightmap's dimensions
    let original_lm_tag = tags.open_tag_copy(&original_lm_tag_path).unwrap();
    let original_lm = original_lm_tag.get_ref::<Bitmap>().unwrap();
    let output_dimensions: Vec<Dimensions> = original_lm.bitmap_data.items.iter().map(|prev_lm_bitmap_data| {
        Dimensions {
            w: prev_lm_bitmap_data.width * scale,
            h: prev_lm_bitmap_data.height * scale,
        }
    }).collect();

    let output_bitmap_data: Vec<Vec<u8>> = bsp.lightmaps.items.iter().enumerate().filter_map(|(i_lightmap, lightmap)| {
        lightmap.bitmap.map(|lm_bitmap_index| {
            let mut verts: Vec<Vert> = Vec::new();
            let mut indices: Vec<u16> = Vec::new();

            lightmap.materials.items.iter().for_each(|material| {
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

            println!("Rendering lightmap {} with {} verts", i_lightmap, verts.len());
            let dimensions = output_dimensions.get(lm_bitmap_index as usize).unwrap();
            renderer.render_randoms(verts, indices, dimensions)
        })
    }).collect();

    let output_lm_tag_path = get_output_lm_tag_path(bsp_tag_path);
    println!("Writing bitmap {}", output_lm_tag_path);
    let pixel_data: Vec<u8> = output_bitmap_data.iter().flatten().map(|&v| v).collect();
    let bitmap = Bitmap {
        _type: BitmapType::_2dTextures,
        encoding_format: BitmapFormat::_16Bit,
        usage: BitmapUsage::LightMap,
        processed_pixel_data: Data::new(pixel_data),
        bitmap_group_sequence: Reflexive::new((0..output_bitmap_data.len()).map(|i| {
            BitmapGroupSequence {
                bitmap_count: 1,
                first_bitmap_index: Some(i as u16),
                ..BitmapGroupSequence::default()
            }
        }).collect()),
        bitmap_data: Reflexive::new(output_bitmap_data.iter().enumerate().map(|(lm_bitmap_index, output_lightmap_data)| {
            let dimensions = output_dimensions.get(lm_bitmap_index).unwrap();
            BitmapData {
                signature: TagGroup::Bitmap,
                width: dimensions.w,
                height: dimensions.h,
                depth: 1,
                _type: BitmapDataType::_2dTexture,
                format: BitmapDataFormat::R5G6B5, //matches renderer OUTPUT_IMAGE_FORMAT
                flags: BitmapDataFlags {
                    power_of_two_dimensions: true,
                    ..BitmapDataFlags::default()
                },
                registration_point: Vector2DInt {
                    x: (dimensions.w / 2) as i16,
                    y: (dimensions.h / 2) as i16,
                },
                mipmap_count: 0,
                pixel_data_offset: (0..lm_bitmap_index)
                    .map(|i| output_bitmap_data[i].len() as u32)
                    .sum(),
                ..BitmapData::default()
            }
        }).collect()),
        ..Bitmap::default()
    };

    tags.write_tag(&output_lm_tag_path, &bitmap).map_err(display_ringhopper_err)?;

    println!("Updating BSP lightmap bitmap reference");
    bsp.lightmaps_bitmap = TagReference::Set(output_lm_tag_path);
    tags.write_tag(bsp_tag_path, bsp).map_err(display_ringhopper_err)?;

    Ok(())
}

fn get_slayer_spawns(scenario: &Scenario) -> Vec<[f32; 3]> {
    scenario.player_starting_locations.items.iter().filter_map(|loc| {
        if is_slayer_spawn(loc.type_0) || is_slayer_spawn(loc.type_1) || is_slayer_spawn(loc.type_2) || is_slayer_spawn(loc.type_3) {
            Some([
                loc.position.x as f32,
                loc.position.y as f32,
                loc.position.z as f32
            ])
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

fn display_ringhopper_err(err: RinghopperError) -> String {
    match err {
        RinghopperError::InvalidTagsDirectory => "Invalid tags directory".into(),
        _ => format!("Unexpected error: {}", err.as_str()),
    }
}