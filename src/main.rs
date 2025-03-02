mod render;

use ringhopper::definitions::{Scenario, ScenarioSpawnType, ScenarioStructureBSP, Bitmap, BitmapType, BitmapFormat, BitmapUsage, BitmapGroupSequence, BitmapData, BitmapDataType, BitmapDataFormat, BitmapDataFlags};
use ringhopper::primitives::parse::{SimpleTagData, TagDataDefaults};
use ringhopper::primitives::primitive::{Data, Reflexive, TagGroup, TagPath, Vector2DInt};
use ringhopper::primitives::tag::{PrimaryTagStruct, PrimaryTagStructDyn};
// use ringhopper::tag::bitmap;
use ringhopper::tag::scenario_structure_bsp::get_uncompressed_vertices_for_bsp_material;
use ringhopper::tag::tree::{TagTree, VirtualTagsDirectory};
use crate::render::{Dimensions, LmRenderer, Vert};

const TAGS_DIR: &str = "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Chelan_1\\tags";

// const SCENARIO_TAG_PATH: &str = "levels\\test\\rock_bottom\\rock_bottom";
// const LM_IN_TAG_PATH: &str = "levels\\test\\rock_bottom\\rock_bottom";
// const LM_OUT_TAG_PATH: &str = "levels\\test\\rock_bottom\\spawns";

const SCENARIO_TAG_PATH: &str = "levels\\test\\bloodgulch\\bloodgulch";
const LM_IN_TAG_PATH: &str = "levels\\test\\bloodgulch\\bloodgulch";
const LM_OUT_TAG_PATH: &str = "levels\\test\\bloodgulch\\spawns";

fn main() {
    let mut tags = VirtualTagsDirectory::new(&[TAGS_DIR], None).unwrap();

    let scenario_tag_path = TagPath::new(SCENARIO_TAG_PATH, TagGroup::Scenario)
        .expect("Invalid scenario tag path");
    let mut scenario_tag = tags.open_tag_copy(&scenario_tag_path).unwrap();
    let scenario = scenario_tag.get_mut::<Scenario>().unwrap();

    let bsp_tag_path = scenario.structure_bsps.items.get(0)
        .expect("Scenario has no BSP")
        .structure_bsp
        .path()
        .expect("BSP tag path is empty");
    let mut bsp_tag = tags.open_tag_copy(bsp_tag_path).unwrap();
    let bsp = bsp_tag.get_mut::<ScenarioStructureBSP>().unwrap();

    let slayer_spawns = get_slayer_spawns(scenario);

    let lm_scale: u16 = 8;
    generate_randoms(&mut tags, slayer_spawns, bsp, lm_scale);
}

fn generate_randoms(tags: &mut VirtualTagsDirectory, slayer_spawns: Vec<[f32; 3]>, bsp: &ScenarioStructureBSP, scale: u16) {
    println!("Generating randoms");
    let renderer = LmRenderer::init(slayer_spawns);

    let prev_lm_bitmap_tag_path = TagPath::new(LM_IN_TAG_PATH, TagGroup::Bitmap).unwrap();
    let prev_lm_bitmap_tag = tags.open_tag_copy(&prev_lm_bitmap_tag_path).unwrap();
    let prev_lm_bitmap = prev_lm_bitmap_tag.get_ref::<Bitmap>().unwrap();
    let output_dimensions: Vec<Dimensions> = prev_lm_bitmap.bitmap_data.items.iter().map(|prev_lm_bitmap_data| {
        Dimensions {
            w: prev_lm_bitmap_data.width * scale,
            h: prev_lm_bitmap_data.height * scale,
        }
    }).collect();

    let output_bitmap_data: Vec<Vec<u8>> = bsp.lightmaps.items.iter().enumerate().filter_map(|(i_lightmap, lightmap)| {
        lightmap.bitmap.map(|lm_bitmap_index| {
            let mut verts: Vec<Vert> = Vec::new();
            let mut indices: Vec<u16> = Vec::new();

            println!("Gathering lightmap {}", i_lightmap);

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

            println!("Rendering lightmap {}", i_lightmap);
            let dimensions = output_dimensions.get(lm_bitmap_index as usize).unwrap();
            renderer.render_randoms(verts, indices, dimensions)
        })
    }).collect();

    println!("Writing bitmap {}", LM_OUT_TAG_PATH);
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
    let bitmap_tag_path = TagPath::new(LM_OUT_TAG_PATH, TagGroup::Bitmap).unwrap();
    tags.write_tag(&bitmap_tag_path, &bitmap).unwrap();
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
