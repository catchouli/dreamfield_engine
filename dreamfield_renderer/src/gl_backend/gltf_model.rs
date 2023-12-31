mod gltf_transform;
mod gltf_mesh;
mod gltf_material;
mod gltf_animation;
mod gltf_skin;
mod gltf_light;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use gl::types::*;
use gltf::json::extras::RawValue;
use gltf::khr_lights_punctual::Kind;
use super::texture::{Texture, TextureParams};
use super::uniform_buffer::{UniformBuffer, GlobalParams, MaterialParams};
use super::{bindings, JointParams, Joint, ToStd140};
use super::lights::LightType;
use cgmath::{Matrix4, Vector3, Matrix};
use serde::{Deserialize, Serialize};

pub use gltf_animation::{GltfAnimation, GltfAnimationKeyframe};
use gltf_transform::{GltfTransformHierarchy, GltfTransform};
use gltf_mesh::GltfMesh;
use gltf_material::GltfMaterial;
use gltf_skin::GltfSkin;
use gltf_light::GltfLight;

/// How many bits to downsample textures to
const TEXTURE_BITS: Option<u8> = Some(5);

/// A gltf model
pub struct GltfModel {
    transform_hierarchy: GltfTransformHierarchy,
    buffers: Vec<u32>,
    drawables: Vec<GltfDrawable>,
    lights: Vec<GltfLight>,
    animations: HashMap<String, GltfAnimation>,
}

/// A single drawable, with a transform, mesh, and optionally skin
pub struct GltfDrawable {
    name: String,
    transform: Option<Arc<Mutex<GltfTransform>>>,
    mesh: Arc<GltfMesh>,
    skin: Option<Arc<Mutex<GltfSkin>>>,
    parsed_extras: GltfNodeExtras,
    raw_extras: Option<Box<RawValue>>
}

/// Extras for a gltf node
#[derive(Serialize, Deserialize, Debug)]
pub struct GltfNodeExtras {
    #[serde(default = "GltfNodeExtras::default_lighting_strength")]
    pub lighting_strength: f32
}

impl GltfNodeExtras {
    fn default_lighting_strength() -> f32 {
        1.0
    }
}

impl Default for GltfNodeExtras {
    fn default() -> Self {
        GltfNodeExtras {
            lighting_strength: Self::default_lighting_strength()
        }
    }
}

impl GltfModel {
    /// Load a model from a gltf file
    pub fn from_file(path: &str) -> Result<GltfModel, gltf::Error> {
        Self::import(gltf::import(path)?)
    }

    /// Load a model from a gltf file embedded in a buffer
    pub fn from_buf(data: &[u8]) -> Result<GltfModel, gltf::Error> {
        Self::import(gltf::import_slice(data)?)
    }

    /// Load from a (doc, buffer_data, image_data)
    /// https://kcoley.github.io/glTF/specification/2.0/figures/gltfOverview-2.0.0a.png
    fn import((doc, buffer_data, image_data): (gltf::Document, Vec<gltf::buffer::Data>, Vec<gltf::image::Data>))
        -> Result<GltfModel, gltf::Error>
    {
        // Load all buffers
        let buffers: Vec<u32> = unsafe {
            let mut buffers = vec![0; buffer_data.len()];
            gl::GenBuffers(buffer_data.len() as i32, buffers.as_mut_ptr());

            for (i, buffer) in buffer_data.iter().enumerate() {
                gl::BindBuffer(gl::ARRAY_BUFFER, buffers[i]);
                gl::BufferData(gl::ARRAY_BUFFER,
                               buffer.len() as GLsizeiptr,
                               buffer.as_ptr() as *const GLvoid,
                               gl::STATIC_DRAW);
            }

            buffers
        };

        // Build transform hierarchy
        let transform_hierarchy = {
            let mut hierarchy = GltfTransformHierarchy::new();
            let root = hierarchy.root().clone();

            for scene in doc.scenes() {
                for node in scene.nodes() {
                    Self::build_hierarchy_recursive(&node, &root, &mut hierarchy)
                }
            }

            hierarchy
        };

        // Load all textures
        let textures = doc.textures()
            .map(|tex| {
                Arc::new(Self::load_texture(&tex, &image_data))
            })
            .collect();

        // Load all materials
        let materials: Vec<Arc<Mutex<GltfMaterial>>> = doc.materials().map(|mat| {
            let mat = GltfMaterial::load(&mat);
            Arc::new(Mutex::new(mat))
        }).collect();

        // Create default material
        let default_material = Arc::new(Mutex::new(GltfMaterial::new()));

        // Load all meshes
        let meshes = doc.meshes().map(|mesh| {
            let mesh = GltfMesh::load(&materials, &default_material, &textures, &mesh, &buffers);
            Arc::new(mesh)
        }).collect();

        // Load all skins
        let skins = doc.skins().map(|skin| {
            let skin = GltfSkin::load(&skin, &buffer_data, &transform_hierarchy);
            Arc::new(Mutex::new(skin))
        }).collect();

        // Load all animations
        let animations = doc.animations().map(|anim| {
            GltfAnimation::load(&anim, &buffer_data, &transform_hierarchy)
        })
        .map(|anim| (anim.name().to_string(), anim))
        .collect();

        // Build scene drawables and lights
        let (drawables, lights) = {
            let mut drawables: Vec<GltfDrawable> = Vec::new();
            let mut lights: Vec<GltfLight> = Vec::new();

            for scene in doc.scenes() {
                for node in scene.nodes() {
                    Self::build_scene_recursive(&node, &transform_hierarchy, &meshes, &skins, None, &mut drawables, &mut lights);
                }
            }

            (drawables, lights)
        };

        Ok(GltfModel {
            transform_hierarchy,
            buffers,
            drawables,
            lights,
            animations
        })
    }

    /// Render a model
    pub fn render(&self, object_world_transform: &Matrix4<f32>, ubo_global: &mut UniformBuffer<GlobalParams>,
        ubo_joints: &mut UniformBuffer<JointParams>, patches: bool)
    {
        // Bind global ubo
        ubo_global.bind(bindings::UniformBlockBinding::GlobalParams);

        // Render all prims
        for drawable in self.drawables.iter() {
            let mesh = &drawable.mesh;
            let model_mat = object_world_transform * drawable.transform
                .as_ref()
                .map(|t| t.lock().unwrap().world_transform().clone())
                .unwrap_or(self.transform_hierarchy.root().lock().unwrap().world_transform().clone());

            // Set model matrix based on whether this is a billboard or not
            if mesh.extras().is_billboard {
                let view_mat = ubo_global.get_mat_view();
                let billboard_mat = Self::calc_billboard_matrix(&view_mat, &model_mat, mesh.extras().keep_upright);
                ubo_global.set_mat_model_derive(&billboard_mat);
            }
            else {
                ubo_global.set_mat_model_derive(&model_mat);
            }

            // Set lighting strength
            let lighting_strength = &drawable.parsed_extras.lighting_strength;
            ubo_global.set_lighting_strength(lighting_strength);

            // Update global uniforms
            ubo_global.upload_changed();

            // Update joint matrices for skinned drawables
            if let Some(skin) = &drawable.skin {
                ubo_joints.set_skinning_enabled(&true);

                for (i, joint) in skin.lock().unwrap().joints().iter().enumerate() {
                    let mut joint_transform = joint.transform().lock().unwrap();
                    let joint_world_transform = object_world_transform * joint_transform.world_transform();

                    let joint_matrix = joint_world_transform * joint.inverse_bind_matrix();
                    ubo_joints.set_joints(i, &Joint {
                        joint_matrix: joint_matrix.to_std140()
                    });
                }
            }
            else {
                ubo_joints.set_skinning_enabled(&false);
            }
            ubo_joints.bind(bindings::UniformBlockBinding::JointParams);

            // Draw mesh
            mesh.draw(patches);
        }
    }

    /// Set the model's transform
    pub fn set_transform(&mut self, transform: &Matrix4<f32>) {
        self.transform_hierarchy.root().lock().unwrap().set_transform(*transform)
    }

    /// Get the drawables list
    pub fn drawables(&self) -> &Vec<GltfDrawable> {
        &self.drawables
    }

    /// Get the model's lights
    pub fn lights(&self) -> &Vec<GltfLight> {
        &self.lights
    }

    /// Get the model's animations
    pub fn animations(&self) -> &HashMap<String, GltfAnimation> {
        &self.animations
    }

    /// Load a gltf texture
    fn load_texture(tex: &gltf::Texture, image_data: &[gltf::image::Data]) -> Texture {
        let data = &image_data[tex.source().index()];
        let sampler = tex.sampler();

        let mut pixels = data.pixels.to_vec();

        // Downsample if enabled
        if let Some(downsample_bits) = TEXTURE_BITS {
            Texture::quantize_to_bit_depth(&mut pixels, downsample_bits);
        }

        let (format, ty, pixels) = (gl::RGBA, gl::UNSIGNED_BYTE, &pixels);
        let dest_format = gl::SRGB8_ALPHA8;

        // Load texture
        let mut tex_params = TextureParams {
            horz_wrap: sampler.wrap_s().as_gl_enum(),
            vert_wrap: sampler.wrap_t().as_gl_enum(),
            min_filter: sampler.min_filter().map(|f| f.as_gl_enum()).unwrap_or(gl::NEAREST),
            mag_filter: sampler.mag_filter().map(|f| f.as_gl_enum()).unwrap_or(gl::NEAREST)
        };

        // TODO: find a way to disable mipmaps in blender's exporter
        tex_params.min_filter = Self::de_mipmapify(tex_params.min_filter);
        tex_params.mag_filter = Self::de_mipmapify(tex_params.mag_filter);

        let width = data.width as i32;
        let height = data.height as i32;
        let tex = Texture::new_from_buf(&pixels, width, height, format, ty, dest_format, tex_params)
            .expect("Failed to load gltf texture");

        // Generate mipmaps - the mag_filter is often on which needs them
        tex.gen_mipmaps();

        tex
    }

    /// build the transform hierarchy
    fn build_hierarchy_recursive(node: &gltf::Node, parent: &Arc<Mutex<GltfTransform>>,
        transform_hierarchy: &mut GltfTransformHierarchy)
    {
        // Get node local transform
        let local_transform = cgmath::Matrix4::from(node.transform().matrix());

        // Create transform node
        let transform = Arc::new(Mutex::new(GltfTransform::from_local(Some(parent.clone()), local_transform)));

        // Recurse into children
        for child in node.children() {
            Self::build_hierarchy_recursive(&child, &transform, transform_hierarchy);
        }

        // Add it to the transform hierarchy
        transform_hierarchy.add_at_index(node.index(), transform);
    }

    /// Build the list of drawables recursively
    fn build_scene_recursive(node: &gltf::Node, transform_hierarchy: &GltfTransformHierarchy,
        meshes: &Vec<Arc<GltfMesh>>, skins: &Vec<Arc<Mutex<GltfSkin>>>, parent_extras: Option<&Box<RawValue>>,
        out_drawables: &mut Vec<GltfDrawable>, out_lights: &mut Vec<GltfLight>)
    {
        let transform = transform_hierarchy.node_by_index(node.index());

        // Get node extras or the parent extras so that they 'inherit' through nodes/collections until overridden
        let node_extras = node.extras().as_ref().or(parent_extras);

        // Load drawable from node
        if let Some(mesh) = node.mesh() {
            // Get skin if there is one
            let name = mesh.name().unwrap_or("").to_string();
            let mesh = meshes[mesh.index()].clone();
            let skin = node.skin().map(|skin| skins[skin.index()].clone());
            let raw_extras = node_extras.map(|raw_value| raw_value.clone());

            // Parse node extras if they're present
            let parsed_extras = node_extras.map(|extras| {
                serde_json::from_str(extras.get()).unwrap()
            }).unwrap_or(Default::default());

            let drawable = GltfDrawable {
                name,
                mesh,
                skin,
                transform: transform.as_ref().map(Clone::clone),
                parsed_extras,
                raw_extras
            };

            // Create drawable
            out_drawables.push(drawable);
        }

        // Load light from node
        if let Some(light) = node.light() {
            let (light_type, inner_cone_angle, outer_cone_angle) = match light.kind() {
                Kind::Directional => (LightType::DirectionalLight, None, None),
                Kind::Point => (LightType::PointLight, None, None),
                Kind::Spot { inner_cone_angle, outer_cone_angle } =>
                    (LightType::SpotLight, Some(inner_cone_angle), Some(outer_cone_angle))
            };

            out_lights.push(GltfLight::new(
                transform.as_ref().map(Clone::clone),
                light_type,
                Vector3::from(light.color()),
                light.intensity(),
                light.range(),
                inner_cone_angle,
                outer_cone_angle
            ));
        }

        // Recurse into children
        for child in node.children() {
            Self::build_scene_recursive(&child, transform_hierarchy, meshes, skins, node_extras, out_drawables,
                out_lights);
        }
    }

    /// Remove mipmap part from a texture filter
    /// TODO: find a way to disable mipmaps in blender's exporter
    fn de_mipmapify(filter: u32) -> u32 {
        match filter {
            gl::NEAREST_MIPMAP_NEAREST => gl::NEAREST,
            gl::LINEAR_MIPMAP_NEAREST => gl::LINEAR,
            gl::NEAREST_MIPMAP_LINEAR => gl::NEAREST,
            gl::LINEAR_MIPMAP_LINEAR => gl::LINEAR,
            _ => filter
        }
    }

    // Calculate a billboard matrix
    fn calc_billboard_matrix(view_mat: &Matrix4<f32>, model_mat: &Matrix4<f32>, keep_upright: bool) -> Matrix4<f32> {
        // Create billboard matrix without object translation
        let mut billboard_mat = match keep_upright {
            false => {
                // Transpose view matrix to get inverse of rotation, and clear view translation
                let mut mat = view_mat.transpose();

                mat[0][3] = 0.0;
                mat[1][3] = 0.0;
                mat[2][3] = 0.0;

                mat
            },
            true => {
                panic!("not implemented: keep_upright");
            }
        };

        // Add model translation
        billboard_mat[3][0] = model_mat[3][0];
        billboard_mat[3][1] = model_mat[3][1];
        billboard_mat[3][2] = model_mat[3][2];

        billboard_mat
    }
}

impl GltfDrawable {
    /// Get the name of the drawable
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the extra fields
    pub fn extras(&self) -> &Option<Box<RawValue>> {
        &self.raw_extras
    }
}

impl Drop for GltfModel {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteVertexArrays(self.buffers.len() as i32, self.buffers.as_ptr());
        }
    }
}
