pub mod input;
pub mod level_collision;

use cgmath::{vec3, Vector3, Zero, InnerSpace};
use crate::renderer::camera::{FpsCamera, Camera};
use crate::renderer::gl_renderer::resources;

use self::input::{InputName, InputState};
use self::level_collision::LevelCollision;

/// The camera look speed
const CAM_LOOK_SPEED: f32 = 1.0;

/// The camera move speed
const CAM_MOVE_SPEED: f32 = 0.1;

/// The camera fast move speed
const CAM_MOVE_SPEED_FAST: f32 = 0.5;

/// The gravity acceleration
const GRAVITY_ACCELERATION: f32 = 0.01;

/// The game state
pub struct GameState {
    pub time: f64,
    pub camera: FpsCamera,
    pub ball_pos: f32,
    pub level_collision: LevelCollision,
    pub velocity: Vector3<f32>
}

impl GameState {
    /// Create a new, default game state
    pub fn new() -> GameState {
        // Create camera
        let camera = FpsCamera::new_with_pos_rot(vec3(0.0, 7.0, 10.0), -0.17, 0.0, CAM_LOOK_SPEED);

        // Load level collision
        let level_collision = LevelCollision::new(resources::MODEL_DEMO_SCENE);

        GameState {
            time: 0.0,
            camera,
            ball_pos: 0.0,
            level_collision,
            velocity: Vector3::zero()
        }
    }

    /// Simulate the game state
    pub fn simulate(&mut self, sim_time: f64, input_state: &InputState) {
        // Update time
        let time_delta = sim_time - self.time;
        self.time = sim_time;

        // Update ball
        self.ball_pos += 0.02;

        // Simulate character
        self.simulate_character(input_state, time_delta as f32);
    }

    /// Get the movement input
    fn movement_input(&mut self, input_state: &InputState) -> (f32, f32) {
        let inputs = input_state.inputs;

        let cam_speed = match inputs[InputName::CamSpeed as usize] {
            false => CAM_MOVE_SPEED,
            true => CAM_MOVE_SPEED_FAST,
        };

        let cam_forwards = inputs[InputName::CamForwards as usize];
        let cam_backwards = inputs[InputName::CamBackwards as usize];
        let cam_left = inputs[InputName::CamLeft as usize];
        let cam_right = inputs[InputName::CamRight as usize];

        let forward_cam_movement = match (cam_forwards, cam_backwards) {
            (true, false) => cam_speed,
            (false, true) => -cam_speed,
            _ => 0.0
        };

        let right_cam_movement = match (cam_left, cam_right) {
            (true, false) => -cam_speed,
            (false, true) => cam_speed,
            _ => 0.0
        };

        (forward_cam_movement, right_cam_movement)
    }

    /// Simulate the character movement
    fn simulate_character(&mut self, input_state: &InputState, time_delta: f32) {
        /// The character camera height
        const CHAR_HEIGHT: f32 = 1.8;

        /// Minimum distance to stop before walls
        const MIN_DIST: f32 = 0.5;

        // Update look direction
        if input_state.cursor_captured {
            let (dx, dy) = input_state.mouse_diff;
            self.camera.mouse_move(dx as f32, dy as f32);
            self.camera.update_matrices();
        }

        // Get camera movement input
        let (forward_cam_movement, right_cam_movement) = self.movement_input(input_state);
        let cam_movement = forward_cam_movement * self.camera.forward() + right_cam_movement * self.camera.right();

        // Update velocity with cam movement and gravity
        self.velocity.x = cam_movement.x;
        self.velocity.z = cam_movement.z;
        self.velocity.y -= GRAVITY_ACCELERATION;

        // Now solve the y movement and xz movement separately
        let mut pos = *self.camera.pos();

        // Resolve vertical motion
        if self.velocity.y < 0.0 {
            let velocity_y_len = f32::abs(self.velocity.y);
            let velocity_y_dir = vec3(0.0, -1.0, 0.0);

            let stop_dist = self.level_collision
                .raycast(&(pos + vec3(0.0, 1.0, 0.0)), &velocity_y_dir, velocity_y_len + 10.0)
                .map(|t| t - CHAR_HEIGHT)
                .filter(|t| *t < velocity_y_len);

            (pos, self.velocity.y) = match stop_dist {
                Some(t) => (pos + t * velocity_y_dir, 0.0),
                _ => (pos + vec3(0.0, self.velocity.y, 0.0), self.velocity.y)
            };
        }

        // Resolve horizontal motion
        let mut movement = vec3(self.velocity.x, 0.0, self.velocity.z);
        for _ in 0..2 {
            if movement.x != 0.0 || movement.y != 0.0 || movement.z != 0.0 {
                movement = self.resolve_movement(&pos, &movement);
            }
        }
        pos += movement;

        // Update camera position
        self.camera.set_pos(&pos);
        self.camera.update();
    }

    fn resolve_movement(&self, pos: &Vector3<f32>, movement: &Vector3<f32>) -> Vector3<f32> {
        let movement_len = movement.magnitude();
        let movement_dir = movement / movement_len;

        let ray_start = pos;
        let ray_dist = movement_len;

        match self.level_collision.raycast_normal(&ray_start, &movement_dir, ray_dist) {
            Some(ray_hit) => {
                let hit_normal = vec3(ray_hit.normal.x, ray_hit.normal.y, ray_hit.normal.z);
                let hit_dot = hit_normal.dot(*movement);
                movement - (hit_dot * hit_normal)
            },
            _ => {
                *movement
            }
        }
    }
}
