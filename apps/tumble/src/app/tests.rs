use super::*;
use crate::scene::HASH_CALLS;

#[test]
fn debug_reading_hashes_only_while_visible() {
    let mut game = Tumble {
        scenes: Scenes::new(),
        commands: 17,
    };
    let mut panel = crcbl::ui::DebugPanel::new();

    HASH_CALLS.with(|calls| calls.set(0));
    game.debug_sections(&mut panel);
    assert!(panel.sections().is_empty());
    HASH_CALLS.with(|calls| assert_eq!(calls.get(), 0));

    for tick in 1..=2 {
        game.scenes.step(1.0 / 60.0);
        let expected_hash = format!("{:016x}", game.scenes.hash());
        HASH_CALLS.with(|calls| calls.set(0));
        panel.set_visible(true);
        panel.begin_frame();
        game.debug_sections(&mut panel);
        HASH_CALLS.with(|calls| assert_eq!(calls.get(), 1));

        let sections = panel.sections();
        assert_eq!(sections.len(), 1);
        let section = &sections[0];
        assert_eq!(section.title(), "physics");
        let value = |label| {
            section
                .rows()
                .iter()
                .find(|row| row.label == label)
                .expect("the physics row")
                .value
                .as_str()
        };
        assert_eq!(value("tick"), tick.to_string());
        assert_eq!(value("hash"), expected_hash);
        assert_eq!(value("commands"), "17");

        panel.set_visible(false);
        panel.begin_frame();
        HASH_CALLS.with(|calls| calls.set(0));
        game.debug_sections(&mut panel);
        assert!(panel.sections().is_empty());
        HASH_CALLS.with(|calls| assert_eq!(calls.get(), 0));
    }
}
