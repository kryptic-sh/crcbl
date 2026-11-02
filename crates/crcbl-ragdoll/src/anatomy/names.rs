//! Humanoid bone names to anatomical roles: Mixamo's, Unreal's and EW's
//! naming, matched without regard to case, then checked against the
//! hierarchy.

/// Which side of the body a bone is on: the character's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

impl Side {
    /// `+1` for the left, `-1` for the right: the sign that mirrors a left
    /// limb's direction onto the right's.
    pub(crate) const fn sign(self) -> f64 {
        match self {
            Self::Left => 1.0,
            Self::Right => -1.0,
        }
    }
}

/// A digit of the hand, by its first segment's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Digit {
    Thumb,
    Index,
    /// A middle, ring or little finger, or EW's `finger` standing for them.
    Other,
}

/// What a bone is, as far as the profile reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Pelvis,
    Spine,
    Neck,
    Head,
    Clavicle(Side),
    UpperArm(Side),
    LowerArm(Side),
    Hand(Side),
    /// A metacarpal, Unreal 5's `index_metacarpal_l` and its kind: part of
    /// the hand.
    Metacarpal(Side),
    /// A segment of a digit, counted from the hand: 0 is the one at the
    /// knuckle (a thumb's at its base, the carpometacarpal joint).
    Finger(Side, Digit, usize),
    Thigh(Side),
    Calf(Side),
    Foot(Side),
    /// The forefoot, from the ball of the foot: its joint is the toes'
    /// metatarsophalangeal joint.
    Ball(Side),
    /// A toe segment beyond the ball.
    Toe(Side),
    /// An IK target or another helper the animation drives: never a body,
    /// nor anything below it.
    Helper,
    /// A twist or corrective bone: never a body, but it may sit in the limb's
    /// chain with the rest of the limb below it.
    Twist,
    /// An eye or an eyebrow: rigid with the head.
    Face,
}

/// A bone's name taken apart: its side, if it names one, and its base
/// lowercased with separators and digits gone.
fn split(name: &str) -> (Option<Side>, String) {
    let mut name = name.to_ascii_lowercase();
    // A namespace, as Mixamo's `mixamorig:`; a Mixamo export without the
    // colon names it as a prefix.
    if let Some(colon) = name.rfind(':') {
        name = name[colon + 1..].to_owned();
    }
    if let Some(rest) = name.strip_prefix("mixamorig") {
        name = rest
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '_')
            .to_owned();
    }
    let mut side = None;
    for (word, named) in [("left", Side::Left), ("right", Side::Right)] {
        if let Some(rest) = name.strip_prefix(word) {
            side = Some(named);
            name = rest.to_owned();
        } else if let Some(rest) = name.strip_suffix(word) {
            side = Some(named);
            name = rest.to_owned();
        }
    }
    if side.is_none() {
        for (suffix, named) in [
            ("_l", Side::Left),
            (".l", Side::Left),
            ("_r", Side::Right),
            (".r", Side::Right),
        ] {
            if let Some(rest) = name.strip_suffix(suffix) {
                side = Some(named);
                name = rest.to_owned();
                break;
            }
        }
    }
    let base = name
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect::<String>();
    (side, base)
}

/// The role a name alone gives, before the hierarchy is consulted; a
/// finger's segment is left at 0 for [`roles`] to count.
fn named(name: &str) -> Option<Role> {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("ik_") || lower.starts_with("ik.") {
        return Some(Role::Helper);
    }
    if lower.contains("twist") || lower.contains("corrective") {
        return Some(Role::Twist);
    }
    let (side, base) = split(name);
    let trunk = |role: Role| side.is_none().then_some(role);
    let limb = |role: fn(Side) -> Role| side.map(role);
    let digit = |digit: Digit| side.map(|side| Role::Finger(side, digit, 0));
    match base.as_str() {
        "pelvis" | "hips" => trunk(Role::Pelvis),
        "spine" | "chest" | "upperchest" => trunk(Role::Spine),
        "neck" => trunk(Role::Neck),
        "head" => trunk(Role::Head),
        "eye" | "eyes" | "eyebrow" | "eyebrows" => Some(Role::Face),
        "clavicle" | "shoulder" => limb(Role::Clavicle),
        "upperarm" | "arm" => limb(Role::UpperArm),
        "lowerarm" | "forearm" => limb(Role::LowerArm),
        "hand" => limb(Role::Hand),
        base if base.ends_with("metacarpal") => limb(Role::Metacarpal),
        "thumb" | "handthumb" => digit(Digit::Thumb),
        "index" | "indexfinger" | "handindex" => digit(Digit::Index),
        "middle" | "handmiddle" | "ring" | "handring" | "pinky" | "handpinky" | "little"
        | "finger" => digit(Digit::Other),
        "thigh" | "upleg" | "upperleg" => limb(Role::Thigh),
        "calf" | "leg" | "lowerleg" | "shin" => limb(Role::Calf),
        "foot" => limb(Role::Foot),
        "ball" | "toebase" => limb(Role::Ball),
        "toe" | "toes" => limb(Role::Toe),
        _ => None,
    }
}

/// Whether `role` may hang below `above`, the nearest ancestor the profile
/// names: the hierarchy's check on a name, so a Mixamo `Leg` is a calf only
/// under a thigh of its side, and a hand only at the end of its forearm.
/// The trunk, and a clavicle or thigh, may hang from anything.
fn fits_under(role: Role, above: Option<Role>) -> bool {
    use Role::{Ball, Calf, Finger, Foot, Hand, LowerArm, Metacarpal, Thigh, Toe, UpperArm};
    match (role, above) {
        (UpperArm(_), Some(above)) => {
            !matches!(above, Thigh(_) | Calf(_) | Foot(_) | Ball(_) | Toe(_))
        }
        (LowerArm(side), Some(above)) => above == UpperArm(side),
        (Hand(side), Some(above)) => above == LowerArm(side),
        (Metacarpal(side), Some(above)) => above == Hand(side),
        (Finger(side, ..), Some(Hand(above) | Metacarpal(above) | Finger(above, ..))) => {
            side == above
        }
        (Calf(side), Some(above)) => above == Thigh(side),
        (Foot(side), Some(above)) => above == Calf(side),
        (Ball(side), Some(above)) => above == Foot(side),
        (Toe(side), Some(Ball(above) | Toe(above) | Foot(above))) => side == above,
        (UpperArm(_), None) => true,
        (
            LowerArm(_) | Hand(_) | Metacarpal(_) | Finger(..) | Calf(_) | Foot(_) | Ball(_)
            | Toe(_),
            _,
        ) => false,
        _ => true,
    }
}

/// Every joint's role, palette order, parents first as a skeleton keeps
/// them: by name, then kept only where the hierarchy agrees, with a
/// finger's segment counted from the hand.
pub(crate) fn roles(names: &[&str], parents: &[Option<usize>]) -> Vec<Option<Role>> {
    let mut roles: Vec<Option<Role>> = Vec::with_capacity(names.len());
    for (joint, name) in names.iter().enumerate() {
        let mut above = parents[joint];
        // The nearest ancestor the profile names, past twist bones in the
        // chain.
        let nearest = loop {
            match above {
                Some(parent) if matches!(roles[parent], None | Some(Role::Twist)) => {
                    above = parents[parent];
                }
                Some(parent) => break roles[parent],
                None => break None,
            }
        };
        let role = named(name).and_then(|role| {
            let role = match (role, nearest) {
                (Role::Finger(side, digit, _), Some(Role::Finger(_, above, segment)))
                    if above == digit =>
                {
                    Role::Finger(side, digit, segment + 1)
                }
                // Toes straight off the foot, with no ball bone: their joint
                // is the ball's.
                (Role::Toe(side), Some(Role::Foot(_))) => Role::Ball(side),
                _ => role,
            };
            fits_under(role, nearest).then_some(role)
        });
        roles.push(role);
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_namings_split_alike() {
        assert_eq!(
            split("mixamorig:LeftUpLeg"),
            (Some(Side::Left), "upleg".into())
        );
        assert_eq!(
            split("mixamorig_RightHandIndex2"),
            (Some(Side::Right), "handindex".into())
        );
        assert_eq!(split("UpperArm_L"), (Some(Side::Left), "upperarm".into()));
        assert_eq!(
            split("indexFinger_03_r"),
            (Some(Side::Right), "indexfinger".into())
        );
        assert_eq!(split("spine_02"), (None, "spine".into()));
        assert_eq!(split("Hips"), (None, "hips".into()));
        assert_eq!(named("ik_hand_gun"), Some(Role::Helper));
        assert_eq!(named("thigh_twist_01_l"), Some(Role::Twist));
        assert_eq!(named("thigh_correctiveRoot_l"), Some(Role::Twist));
        assert_eq!(
            named("index_metacarpal_l"),
            Some(Role::Metacarpal(Side::Left))
        );
        assert_eq!(named("mixamorig:LeftEye"), Some(Role::Face));
        assert_eq!(named("eyebrows"), Some(Role::Face));
        // A trunk name with a side, and a limb name without one, are not
        // what they seem.
        assert_eq!(named("spine_l"), None);
        assert_eq!(named("calf"), None);
    }

    #[test]
    fn the_hierarchy_counts_segments_and_refuses_a_misplaced_name() {
        let names = [
            "pelvis",
            "thigh_l",
            "calf_l",
            "foot_l",
            "lowerarm_l",
            "hand_r",
            "index_01_l",
        ];
        let parents = [None, Some(0), Some(1), Some(2), Some(0), Some(0), Some(5)];
        let found = roles(&names, &parents);
        assert_eq!(found[2], Some(Role::Calf(Side::Left)));
        assert_eq!(found[3], Some(Role::Foot(Side::Left)));
        // A forearm on the pelvis, a hand with no forearm, and a finger on
        // the other side's hand are left unnamed.
        assert_eq!(found[4], None);
        assert_eq!(found[5], None);
        assert_eq!(found[6], None);

        let names = ["Hand_L", "thumb_01_l", "thumb_02_l", "thumb_03_l"];
        let parents = [None, Some(0), Some(1), Some(2)];
        let found = roles(&names, &parents);
        assert_eq!(found[0], None, "a hand with no forearm above it");
        let names = [
            "UpperArm_L",
            "lowerarm_l",
            "Hand_L",
            "thumb_01_l",
            "thumb_02_l",
            "thumb_03_l",
        ];
        let parents = [None, Some(0), Some(1), Some(2), Some(3), Some(4)];
        let found = roles(&names, &parents);
        assert_eq!(found[2], Some(Role::Hand(Side::Left)));
        assert_eq!(found[5], Some(Role::Finger(Side::Left, Digit::Thumb, 2)));
    }
}
