// @generated automatically by Diesel CLI.

diesel::table! {
    channel_members (channel_id, user_id) {
        channel_id -> Text,
        user_id -> Text,
        role -> Text,
        joined_at_ms -> Int8,
        last_read_seq -> Int8,
        last_read_at_ms -> Int8,
        notify -> Text,
    }
}

diesel::table! {
    channels (id) {
        id -> Text,
        workspace_id -> Text,
        kind -> Text,
        slug -> Nullable<Text>,
        name -> Text,
        topic -> Nullable<Text>,
        head_seq -> Int8,
        created_by -> Text,
        created_at_ms -> Int8,
        archived_at_ms -> Nullable<Int8>,
    }
}

diesel::table! {
    dm_pairs (pair_hash) {
        pair_hash -> Bytea,
        channel_id -> Text,
    }
}

diesel::table! {
    events (workspace_id, pts, created_at_ms) {
        pts -> Int8,
        workspace_id -> Text,
        qts -> Int8,
        user_id -> Nullable<Text>,
        channel_id -> Nullable<Text>,
        channel_seq -> Nullable<Int8>,
        scope -> Text,
        body_kind -> Text,
        body -> Bytea,
        created_at_ms -> Int8,
        delivered_at_ms -> Nullable<Int8>,
    }
}

diesel::table! {
    events_default (workspace_id, pts, created_at_ms) {
        pts -> Int8,
        workspace_id -> Text,
        qts -> Int8,
        user_id -> Nullable<Text>,
        channel_id -> Nullable<Text>,
        channel_seq -> Nullable<Int8>,
        scope -> Text,
        body_kind -> Text,
        body -> Bytea,
        created_at_ms -> Int8,
        delivered_at_ms -> Nullable<Int8>,
    }
}

diesel::table! {
    mentions (message_id, user_id, mention_kind) {
        message_id -> Text,
        user_id -> Text,
        mention_kind -> Text,
    }
}

diesel::table! {
    messages (id) {
        id -> Text,
        channel_id -> Text,
        seq -> Int8,
        author_id -> Text,
        parent_id -> Nullable<Text>,
        client_msg_id -> Text,
        body -> Text,
        body_format -> Text,
        mentions -> Jsonb,
        attachments -> Jsonb,
        thread_reply_count -> Int8,
        thread_last_seq -> Nullable<Int8>,
        created_at_ms -> Int8,
        edited_at_ms -> Nullable<Int8>,
        deleted_at_ms -> Nullable<Int8>,
    }
}

diesel::table! {
    reactions (message_id, user_id, emoji) {
        message_id -> Text,
        user_id -> Text,
        emoji -> Text,
        created_at_ms -> Int8,
    }
}

diesel::table! {
    sessions (id) {
        id -> Text,
        user_id -> Text,
        refresh_secret_hash -> Bytea,
        last_seen_ms -> Int8,
        revoked_at_ms -> Nullable<Int8>,
        client_info -> Text,
    }
}

diesel::table! {
    users (id) {
        id -> Text,
        email -> Citext,
        password_hash -> Nullable<Text>,
        display_name -> Text,
        avatar_url -> Nullable<Text>,
        status -> Text,
        created_at_ms -> Int8,
        updated_at_ms -> Int8,
    }
}

diesel::table! {
    workspace_invites (id) {
        id -> Text,
        workspace_id -> Text,
        code -> Text,
        email -> Nullable<Text>,
        role -> Text,
        expires_at_ms -> Int8,
        redeemed_by -> Nullable<Text>,
        redeemed_at_ms -> Nullable<Int8>,
        created_by -> Text,
        created_at_ms -> Int8,
    }
}

diesel::table! {
    workspace_members (id) {
        id -> Text,
        workspace_id -> Text,
        user_id -> Text,
        role -> Text,
        display_name -> Nullable<Text>,
        joined_at_ms -> Int8,
        removed_at_ms -> Nullable<Int8>,
        head_qts -> Int8,
    }
}

diesel::table! {
    workspaces (id) {
        id -> Text,
        slug -> Text,
        name -> Text,
        icon_url -> Nullable<Text>,
        created_by -> Text,
        created_at_ms -> Int8,
        head_pts -> Int8,
        min_available_pts -> Int8,
    }
}

diesel::joinable!(channel_members -> channels (channel_id));
diesel::joinable!(channel_members -> users (user_id));
diesel::joinable!(channels -> users (created_by));
diesel::joinable!(channels -> workspaces (workspace_id));
diesel::joinable!(dm_pairs -> channels (channel_id));
diesel::joinable!(mentions -> messages (message_id));
diesel::joinable!(messages -> channels (channel_id));
diesel::joinable!(messages -> users (author_id));
diesel::joinable!(reactions -> messages (message_id));
diesel::joinable!(reactions -> users (user_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(workspace_invites -> workspaces (workspace_id));
diesel::joinable!(workspace_members -> users (user_id));
diesel::joinable!(workspace_members -> workspaces (workspace_id));
diesel::joinable!(workspaces -> users (created_by));

diesel::allow_tables_to_appear_in_same_query!(
    channel_members,
    channels,
    dm_pairs,
    events,
    events_default,
    mentions,
    messages,
    reactions,
    sessions,
    users,
    workspace_invites,
    workspace_members,
    workspaces,
);
