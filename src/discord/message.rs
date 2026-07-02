mod info;
mod state;

pub use info::{
    AttachmentInfo, AttachmentUpdate, EmbedFieldInfo, EmbedInfo, InlinePreviewInfo,
    MESSAGE_FLAG_SUPPRESS_EMBEDS, MentionInfo, MessageInfo, MessageInteractionInfo, MessageKind,
    MessageReferenceInfo, MessageSnapshotInfo, PollAnswerInfo, PollInfo, ReactionInfo,
    ReactionUserInfo, ReplyInfo,
};
pub(in crate::discord) use state::{MessageAuthorRoleIds, MessageHistoryGap, MessageUpdateFields};
pub use state::{MessageCapabilities, MessageState};
