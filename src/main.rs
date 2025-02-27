mod render;

use std::fs;
use std::path::Path;
use ringhopper::definitions::{
    Scenario,
    ScenarioSpawnType,
    ScenarioStructureBSP,
    Bitmap
};
use ringhopper::primitives::parse::{SimpleTagData, TagDataDefaults};
use ringhopper::primitives::primitive::{TagGroup, TagPath};
use ringhopper::primitives::tag::{PrimaryTagStruct, PrimaryTagStructDyn};
use ringhopper::tag::scenario_structure_bsp::get_uncompressed_vertices_for_bsp_material;
use ringhopper::tag::tree::{TagTree, VirtualTagsDirectory};
use render::{render_lm_randoms, LmVert};
use crate::render::Dimensions;

const TAGS_DIR: &str = "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Chelan_1\\tags";
const SCENARIO_TAG_PATH: &str = "levels\\test\\bloodgulch\\bloodgulch";

fn main() {
    let tags = VirtualTagsDirectory::new(&[TAGS_DIR], None).unwrap();

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

    generate_randoms(slayer_spawns, bsp);
}

fn generate_randoms(slayer_spawns: Vec<[f32; 3]>, bsp: &ScenarioStructureBSP) {
    let lightmap_index = 1;
    let material_index = 1;

    let lightmap = bsp.lightmaps.items.get(1).unwrap();
    let material = lightmap.materials.items.get(1).unwrap();
    println!("Rendering randoms in lightmap {} material {}", lightmap_index, material_index);

    let (rendered_verts, lm_verts) = get_uncompressed_vertices_for_bsp_material(material).unwrap();
    let rendered_verts = rendered_verts.collect::<Vec<_>>();
    let lm_verts: Vec<LmVert> = lm_verts
        .enumerate()
        .map(|(i, v)| {
            let rendered_vert = rendered_verts.get(i).unwrap();
            LmVert {
                lm_uv: [
                    v.texture_coords.x as f32,
                    v.texture_coords.y as f32
                ],
                world_pos: [
                    rendered_vert.position.x as f32,
                    rendered_vert.position.y as f32,
                    rendered_vert.position.z as f32,
                ],
            }
        })
        .collect();

    let lm_indices: Vec<u16> = (material.surfaces..(material.surfaces + material.surface_count))
        .flat_map(|surface_index| {
            let bsp_surface = bsp.surfaces.items.get(surface_index as usize).unwrap();
            [bsp_surface.vertex0_index.unwrap(), bsp_surface.vertex1_index.unwrap(), bsp_surface.vertex2_index.unwrap()]
        })
        .collect();

    let dimensions = Dimensions {
        w: 128 * 2,
        h: 256 * 2,
    };

    render_lm_randoms(slayer_spawns, lm_verts, lm_indices, dimensions);
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
