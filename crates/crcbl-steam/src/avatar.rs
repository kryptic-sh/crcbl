//! Avatars: a user's picture as RGBA pixels.
//!
//! `ISteamFriends` hands out an image *handle* per avatar size, and
//! `ISteamUtils` reads a handle's size and pixels. Handle `0` means the user
//! has no avatar of that size, and `-1` that the large one is still
//! downloading; both are `Ok(None)` here, and neither reaches `ISteamUtils`.
//! A download that finishes arrives as
//! [`SteamEvent::AvatarLoaded`](crate::SteamEvent::AvatarLoaded).

use crate::{Friends, SteamId, error::SteamError, matchmaking::refused_unless};

/// Which of the three avatar sizes Steam keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AvatarSize {
    /// 32 × 32 (`GetSmallFriendAvatar`).
    Small,
    /// 64 × 64 (`GetMediumFriendAvatar`).
    Medium,
    /// 184 × 184 (`GetLargeFriendAvatar`), which may still be downloading.
    Large,
}

/// An image's pixels, row by row, four bytes per pixel in R, G, B, A order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `4 × width × height` bytes.
    pub pixels: Vec<u8>,
}

/// The bytes an RGBA image of `width × height` needs, if that fits in both a
/// `usize` and the `int` `GetImageRGBA` takes.
fn rgba_len(width: u32, height: u32) -> Option<(usize, i32)> {
    let len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    Some((len, i32::try_from(len).ok()?))
}

impl Friends<'_> {
    /// A user's avatar at `size`: `Ok(None)` when they have none of that size,
    /// or it is still downloading.
    ///
    /// # Errors
    ///
    /// [`SteamError::Refused`] when Steam will not size or copy the image, and
    /// [`SteamError::ImageTooLarge`] when the size it reports cannot be
    /// allocated or handed back — refused before any pixel is copied.
    pub fn avatar(&self, user: SteamId, size: AvatarSize) -> Result<Option<Rgba>, SteamError> {
        let steam = self.steam();
        let client = &steam.client;
        let fns = &client.lib.fns;
        let get = match size {
            AvatarSize::Small => fns.friends.get_small_friend_avatar,
            AvatarSize::Medium => fns.friends.get_medium_friend_avatar,
            AvatarSize::Large => fns.friends.get_large_friend_avatar,
        };
        // SAFETY: `client.friends` is the non-null interface init resolved.
        let image = unsafe { get(client.friends, user.0) };
        if image == 0 || image == -1 {
            return Ok(None);
        }
        let (mut width, mut height) = (0_u32, 0_u32);
        // SAFETY: `client.utils` is the non-null interface init resolved;
        // `width` and `height` are writable for the call.
        let sized = unsafe {
            (fns.utils.get_image_size)(client.utils, image, &raw mut width, &raw mut height)
        };
        refused_unless(sized, "GetImageSize")?;
        let Some((len, capacity)) = rgba_len(width, height) else {
            return Err(SteamError::ImageTooLarge { width, height });
        };
        let mut pixels = vec![0_u8; len];
        // SAFETY: as above; `pixels` is `capacity` writable bytes.
        let copied = unsafe {
            (fns.utils.get_image_rgba)(client.utils, image, pixels.as_mut_ptr(), capacity)
        };
        refused_unless(copied, "GetImageRGBA")?;
        Ok(Some(Rgba {
            width,
            height,
            pixels,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppId, client::init_on, testing};

    #[test]
    fn the_buffer_is_four_bytes_a_pixel_and_overflow_is_none() {
        assert_eq!(rgba_len(32, 32), Some((4096, 4096)));
        assert_eq!(rgba_len(0, 64), Some((0, 0)));
        // Past the `int` GetImageRGBA takes: 4 × 32768 × 16384 = 2^31.
        assert_eq!(rgba_len(32_768, 16_384), None);
        assert_eq!(rgba_len(u32::MAX, u32::MAX), None);
    }

    #[test]
    fn an_avatar_is_sized_then_copied() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| {
            s.avatar_handles = [11, 12, 13];
            s.image = Some((2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]));
        });
        let avatar = steam
            .friends()
            .avatar(SteamId(7), AvatarSize::Medium)
            .unwrap();
        assert_eq!(
            avatar,
            Some(Rgba {
                width: 2,
                height: 1,
                pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
            })
        );
        assert_eq!(
            testing::script(|s| s.image_calls.clone()),
            [("GetImageSize", 12, 0), ("GetImageRGBA", 12, 8)]
        );
    }

    #[test]
    fn no_avatar_and_a_download_in_flight_are_none_and_never_reach_utils() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| {
            s.avatar_handles = [0, 0, -1];
            s.image = Some((2, 1, vec![0; 8]));
        });
        for size in [AvatarSize::Small, AvatarSize::Large] {
            assert_eq!(steam.friends().avatar(SteamId(7), size), Ok(None));
        }
        assert!(testing::script(|s| s.image_calls.is_empty()));
    }

    #[test]
    fn an_image_too_large_to_hand_back_is_refused_before_copying() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| {
            s.avatar_handles = [11, 12, 13];
            s.image = Some((32_768, 16_384, Vec::new()));
        });
        assert!(matches!(
            steam.friends().avatar(SteamId(7), AvatarSize::Small),
            Err(SteamError::ImageTooLarge {
                width: 32_768,
                height: 16_384
            })
        ));
        assert_eq!(
            testing::script(|s| s.image_calls.clone()),
            [("GetImageSize", 11, 0)],
            "nothing was copied"
        );
    }

    #[test]
    fn a_refused_size_or_copy_is_an_error_naming_it() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        testing::script(|s| s.avatar_handles = [11, 12, 13]);
        assert_eq!(
            steam.friends().avatar(SteamId(7), AvatarSize::Small),
            Err(SteamError::Refused("GetImageSize"))
        );
        testing::script(|s| {
            s.image = Some((1, 1, vec![0; 4]));
            s.refuse = true;
        });
        assert_eq!(
            steam.friends().avatar(SteamId(7), AvatarSize::Small),
            Err(SteamError::Refused("GetImageRGBA"))
        );
    }
}
