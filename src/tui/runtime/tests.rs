use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    DashboardState, apply_file_picker_selection, decode_file_picker_output, file_picker_command,
    input,
};
use crate::discord::ids::Id;
use crate::discord::test_builders::{
    GuildCreateFixture, MessageHistoryLoadedFixture, guild_create_event,
    message_history_loaded_event,
};
use crate::discord::{AppCommand, AppEvent, ChannelInfo, MessageInfo};
use crate::tui::state::FocusPane;

fn state_with_channel(kind: &str) -> DashboardState {
    let user_id = Id::new(10);
    let guild_id = Id::new(1);
    let channel_id = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(user_id),
    });
    state.push_event(guild_create_event(GuildCreateFixture {
        owner_id: Some(user_id),
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "uploads".to_owned(),
            ..ChannelInfo::test(channel_id, kind)
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    assert!(state.select_visible_pane_row(FocusPane::Guilds, 1));
    assert!(state.confirm_selected_guild());
    state.confirm_selected_channel();
    assert_eq!(state.selected_channel_id(), Some(channel_id));
    state.push_event(message_history_loaded_event(MessageHistoryLoadedFixture {
        channel_id,
        messages: vec![MessageInfo::test(channel_id, Id::new(100))],
        ..MessageHistoryLoadedFixture::new()
    }));
    state.start_composer();
    state
}

#[test]
fn file_picker_adds_selected_files_to_message_draft() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let paths = ["first note.txt", "second note.txt"].map(|name| directory.path().join(name));
    for path in &paths {
        std::fs::write(path, b"attachment").expect("write attachment");
    }
    let mut state = state_with_channel("GuildText");
    assert!(state.is_composing());
    input::handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert!(state.take_open_file_picker_request());

    apply_file_picker_selection(
        &mut state,
        paths
            .iter()
            .map(|path| path.to_str().expect("UTF-8 path").to_owned())
            .collect(),
    );

    let attachments = state.pending_composer_attachments();
    assert_eq!(attachments.len(), 2);
    for (attachment, path) in attachments.iter().zip(&paths) {
        assert_eq!(attachment.path(), Some(path.as_path()));
        assert_eq!(attachment.size_bytes, 10);
    }
}

#[test]
fn file_picker_adds_selected_file_to_forum_post() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("forum note.txt");
    std::fs::write(&path, b"attachment").expect("write attachment");
    let mut state = state_with_channel("forum");
    state.activate_forum_post_composer();
    state.insert_forum_post_text("A post");
    state.activate_forum_post_composer();
    state.cycle_forum_post_field_next();
    state.activate_forum_post_composer();
    state.insert_forum_post_text("Post body");
    input::handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    );
    assert!(state.take_open_file_picker_request());

    apply_file_picker_selection(
        &mut state,
        vec![path.to_str().expect("UTF-8 path").to_owned()],
    );
    state.activate_forum_post_composer();

    let Some(AppCommand::CreateForumPost { post }) = state.save_forum_post_composer() else {
        panic!("forum post should be ready to send");
    };
    assert_eq!(post.attachments.len(), 1);
    assert_eq!(post.attachments[0].path(), Some(path.as_path()));
    assert_eq!(post.attachments[0].filename, "forum note.txt");
    assert_eq!(post.attachments[0].size_bytes, 10);
}

#[test]
fn file_picker_custom_command_writes_selected_paths() {
    let output = tempfile::Builder::new()
        .prefix("concord picker 출력-")
        .tempfile()
        .expect("picker output file")
        .into_temp_path();
    #[cfg(not(windows))]
    let picker = r#"test "$1" = "$CONCORD_PICKER_OUTPUT" && printf '%s\n' '첨부 file.txt' > "$1""#;
    #[cfg(windows)]
    let picker = "'첨부 file.txt' > $env:CONCORD_PICKER_OUTPUT";

    let status = file_picker_command(picker, &output)
        .status()
        .expect("run custom picker");
    assert!(status.success());
    let bytes = std::fs::read(&output).expect("read picker output");
    assert_eq!(
        decode_file_picker_output(&bytes).expect("decode picker output"),
        ["첨부 file.txt"],
    );
}

#[test]
fn file_picker_decodes_supported_output_encodings() {
    let content = " résumé 📎.txt \r\n\r\nfolder/다음 file.txt\n";
    let utf16le_bom = [0xFF, 0xFE]
        .into_iter()
        .chain(content.encode_utf16().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
    let cases = [
        ("UTF-8", content.as_bytes().to_vec()),
        (
            "UTF-8 with BOM",
            [b"\xEF\xBB\xBF", content.as_bytes()].concat(),
        ),
        ("UTF-16LE with BOM", utf16le_bom),
    ];

    for (encoding, bytes) in cases {
        assert_eq!(
            decode_file_picker_output(&bytes).expect(encoding),
            [" résumé 📎.txt ", "folder/다음 file.txt"],
            "{encoding}",
        );
    }
}
