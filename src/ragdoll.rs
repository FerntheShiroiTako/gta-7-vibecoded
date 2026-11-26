//! Physics ragdolls: the animated limb pivots of a humanoid are detached and
//! turned into Rapier rigid bodies linked by spherical joints, keeping their
//! current pose and the velocity of the hit.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::humanoid::{Limb, Rig};
use crate::vehicle::{GROUP_VEHICLE, GROUP_WORLD};

pub struct RagdollPlugin;

impl Plugin for RagdollPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, begin_ragdolls.after(bevy::transform::TransformSystems::Propagate));
    }
}

pub const GROUP_RAGDOLL: Group = Group::GROUP_4;

/// Insert on a humanoid visual root to turn it into a ragdoll.
#[derive(Component)]
pub struct WantsRagdoll {
    pub vel: Vec3,
}

/// The detached body parts (despawn them together with the owner).
#[derive(Component)]
pub struct RagdollParts(pub Vec<Entity>);

fn begin_ragdolls(
    mut commands: Commands,
    roots: Query<(Entity, &WantsRagdoll, &Children), With<Rig>>,
    children_q: Query<&Children>,
    limbs: Query<(&Limb, &GlobalTransform)>,
) {
    for (root, want, children) in &roots {
        // collect limb pivots of this rig
        let mut found: HashMap<Limb, (Entity, GlobalTransform)> = HashMap::new();
        let mut stack: Vec<Entity> = children.iter().collect();
        while let Some(e) = stack.pop() {
            if let Ok(ch) = children_q.get(e) {
                stack.extend(ch.iter());
            }
            if let Ok((l, gt)) = limbs.get(e) {
                found.insert(*l, (e, *gt));
            }
        }
        commands.entity(root).remove::<WantsRagdoll>();
        let Some(&(body, body_gt)) = found.get(&Limb::Body) else { continue };
        // (limb, parent, collider half-length, radius)
        let spec = [
            (Limb::Body, None, 0.0, 0.0),
            (Limb::ArmL, Some(Limb::Body), 0.15, 0.055),
            (Limb::ArmR, Some(Limb::Body), 0.15, 0.055),
            (Limb::ForearmL, Some(Limb::ArmL), 0.16, 0.045),
            (Limb::ForearmR, Some(Limb::ArmR), 0.16, 0.045),
            (Limb::LegL, Some(Limb::Body), 0.21, 0.075),
            (Limb::LegR, Some(Limb::Body), 0.21, 0.075),
            (Limb::ShinL, Some(Limb::LegL), 0.24, 0.06),
            (Limb::ShinR, Some(Limb::LegR), 0.24, 0.06),
        ];
        let mut parts = Vec::new();
        for (limb, parent, half, r) in spec {
            let Some(&(e, gt)) = found.get(&limb) else { continue };
            let collider = if limb == Limb::Body {
                // pelvis..head in one capsule (the Body pivot sits at foot level)
                Collider::compound(vec![(Vec3::new(0.0, 1.27, 0.0), Quat::IDENTITY, Collider::capsule_y(0.36, 0.17))])
            } else {
                Collider::compound(vec![(Vec3::new(0.0, -half, 0.0), Quat::IDENTITY, Collider::capsule_y((half - r).max(0.02), r))])
            };
            let world = gt.compute_transform();
            let mut ec = commands.entity(e);
            ec.remove::<(Limb, ChildOf)>().insert((
                world,
                RigidBody::Dynamic,
                collider,
                ColliderMassProperties::Density(950.0),
                Velocity { linear: want.vel, angular: Vec3::ZERO },
                Damping { linear_damping: 0.15, angular_damping: 1.2 },
                Friction::coefficient(0.9),
                CollisionGroups::new(GROUP_RAGDOLL, GROUP_WORLD | GROUP_VEHICLE),
                ExternalImpulse::default(),
                ReadMassProperties::default(),
                Ccd::enabled(),
            ));
            if let Some(pl) = parent {
                if let Some(&(pe, pgt)) = found.get(&pl) {
                    let anchor = pgt.affine().inverse().transform_point3(gt.translation());
                    let joint = SphericalJointBuilder::new().local_anchor1(anchor).local_anchor2(Vec3::ZERO);
                    ec.insert(ImpulseJoint::new(pe, joint));
                }
            }
            parts.push(e);
        }
        let _ = body_gt;
        let _ = body;
        commands.entity(root).insert(RagdollParts(parts));
    }
}
