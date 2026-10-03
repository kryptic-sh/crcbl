//! The scene's environment — `env.ron`'s camera and ambient light — edited
//! as a component's fields are: drawn by the inspector while nothing is
//! selected, written through [`EditCommand::SetEnvironment`], and copied and
//! pasted one leaf at a time.
//!
//! Before this a scene from empty kept the compiled-in scene's environment
//! for good, since nothing in the editor wrote one: towers' field, whose
//! `env.ron` holds the game's own camera and ambient, could not be made from
//! empty without a text editor.
//!
//! # A mirror, not the scene's own type
//!
//! [`Env`] is `crcbl-scene`'s, which knows nothing of reflection, so the
//! fields a panel draws and a command's path names are [`Environment`]'s,
//! rebuilt from the scene's [`Env`] each time it is read and written back
//! whole after each write. A write reads the leaf first and hands back the
//! command that puts it back, as [`crate::command::set_property`] does for a
//! component.
//!
//! **Flat, where the file nests the camera's two points**: a nested struct is
//! a collapsing group in the inspector, shut until it is clicked open, and
//! three rows of three numbers need no hiding. So `camera.1` is the file's
//! `camera: Camera(position: (_, y, _))` and `look_at.1` its
//! `look_at: (_, y, _)`.
//!
//! # No rewind
//!
//! The inspector edits the [`Environment`] it is handed, which is a copy, so
//! the scene is untouched when the edit is reported and
//! [`Document::record_environment`] has nothing to put back before it
//! applies the command — unlike [`Document::record_edits`], whose panel
//! writes into the world itself.

use crcbl::reflect::{Reflect, Value, get_path, set_path};
use crcbl::scene::scn::{Env, EnvCamera};
use crcbl::ui::tree::FieldEdit;

use super::field::{text_of, value_of};
use super::{Document, EditError};
use crate::command::{EditCommand, Gesture};

/// `env.ron`, as the inspector draws it and a command's path names it.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
pub struct Environment {
    /// Where the scene is viewed from when it opens: the camera's eye, in
    /// world space, in metres.
    #[reflect(name = "Camera", step = 0.1)]
    pub camera: [f32; 3],
    /// The world-space point the camera is aimed at, in metres.
    #[reflect(name = "Looks at", step = 0.1)]
    pub look_at: [f32; 3],
    /// Linear RGB the scene is lit by in the absence of any other light.
    #[reflect(name = "Ambient", min = 0.0, max = 1.0, step = 0.01)]
    pub ambient: [f32; 3],
}

impl From<&Env> for Environment {
    fn from(env: &Env) -> Self {
        Self {
            camera: env.camera.position,
            look_at: env.camera.look_at,
            ambient: env.ambient,
        }
    }
}

impl From<Environment> for Env {
    fn from(environment: Environment) -> Self {
        Self {
            camera: EnvCamera {
                position: environment.camera,
                look_at: environment.look_at,
            },
            ambient: environment.ambient,
        }
    }
}

impl Document {
    /// The scene's environment, as the inspector draws it — a copy: edits go
    /// through [`record_environment`](Self::record_environment) or an
    /// [`EditCommand::SetEnvironment`].
    #[must_use]
    pub fn environment(&self) -> Environment {
        Environment::from(self.scene.env())
    }

    /// The leaf `path` names inside the scene's [`Environment`].
    ///
    /// # Errors
    ///
    /// [`EditError::Path`] for a path that names nothing or stops short of a
    /// leaf.
    pub fn read_environment(&self, path: &str) -> Result<Value, EditError> {
        Ok(get_path(&self.environment(), path)?)
    }

    /// Turns the edits the inspector made to a copy of the scene's
    /// [`Environment`] in one frame into one command — an
    /// [`EditCommand::SetEnvironment`] for one leaf, a batch of them for
    /// several — recorded through [`apply_in`](Self::apply_in) under
    /// `gesture` when there is one, so a drag is one entry. See the module
    /// docs for why nothing is rewound first.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode, and [`EditError::Path`] for a path
    /// that names no leaf or a value the leaf refuses. Nothing is written or
    /// recorded when it refuses.
    pub fn record_environment(
        &mut self,
        edits: &[FieldEdit],
        gesture: Option<Gesture>,
    ) -> Result<(), EditError> {
        if edits.is_empty() {
            return Ok(());
        }
        let command = EditCommand::one_or_batch(
            edits
                .iter()
                .map(|edit| EditCommand::SetEnvironment {
                    path: edit.path.clone(),
                    value: edit.after.clone(),
                })
                .collect(),
        );
        match gesture {
            Some(gesture) => self.apply_in(command, gesture),
            None => self.apply(command),
        }
    }

    /// The text of the leaf `path` names inside the scene's [`Environment`],
    /// as `env.ron` spells it — the field half of the clipboard, as
    /// [`copy_field`](Self::copy_field) is for a component.
    ///
    /// # Errors
    ///
    /// As [`read_environment`](Self::read_environment).
    pub fn copy_environment_field(&self, path: &str) -> Result<String, EditError> {
        Ok(text_of(&self.read_environment(path)?))
    }

    /// Writes the value `text` spells into the leaf `path` names inside the
    /// scene's [`Environment`], as one [`EditCommand::SetEnvironment`] — read
    /// as [`paste_field`](Self::paste_field) reads a component's leaf.
    ///
    /// # Errors
    ///
    /// [`EditError::Playing`] in play mode; [`EditError::Path`] as
    /// [`read_environment`](Self::read_environment), and again for a value
    /// the leaf refuses; [`EditError::FieldPaste`] for text that is not a
    /// value of the leaf's kind. The environment is unchanged and nothing is
    /// recorded in every case.
    pub fn paste_environment_field(&mut self, path: &str, text: &str) -> Result<(), EditError> {
        self.refuse_in_play()?;
        let current = self.read_environment(path)?;
        let value = value_of(&current, text).map_err(|error| EditError::FieldPaste {
            path: path.to_owned(),
            message: error.code.to_string(),
        })?;
        self.apply(EditCommand::SetEnvironment {
            path: path.to_owned(),
            value,
        })
    }

    /// Writes `value` into the leaf `path` names inside the scene's
    /// [`Environment`], and hands back the [`EditCommand::SetEnvironment`]
    /// that puts back the value it replaced — the body of that command.
    pub(super) fn set_environment(
        &mut self,
        path: &str,
        value: &Value,
    ) -> Result<EditCommand, EditError> {
        let mut environment = self.environment();
        let replaced = get_path(&environment, path)?;
        set_path(&mut environment, path, value)?;
        *self.scene.env_mut() = environment.into();
        Ok(EditCommand::SetEnvironment {
            path: path.to_owned(),
            value: replaced,
        })
    }
}
