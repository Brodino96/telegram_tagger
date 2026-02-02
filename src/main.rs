mod db;

use std::sync::Arc;
use teloxide::{
    prelude::*,
    types::{ChatMemberKind, ParseMode, ReplyParameters},
    utils::command::BotCommands,
};
use tokio::sync::Mutex;

type Db = Arc<Mutex<rusqlite::Connection>>;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum Command {
    #[command(description = "Tag all users in the group")]
    All(String),
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    pretty_env_logger::init();
    log::info!("Starting tagger bot...");

    let conn = db::init_db().expect("Failed to initialize database");
    log::info!("Database initialized successfully");
    let db: Db = Arc::new(Mutex::new(conn));

    let bot = Bot::from_env();
    log::info!("Bot created, starting dispatcher...");

    let handler = dptree::entry()
        // Handle user joins/leaves
        .branch(Update::filter_chat_member().endpoint(chat_member_handler))
        // Handle messages
        .branch(
            Update::filter_message()
                .branch(
                    dptree::entry()
                        .filter_command::<Command>()
                        .endpoint(command_handler),
                )
                .branch(dptree::endpoint(message_handler)),
        );

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

/// Handles chat member updates, tracks users joining the group
async fn chat_member_handler(_bot: Bot, update: ChatMemberUpdated, db: Db) -> ResponseResult<()> {
    let chat_id = update.chat.id.0;
    let chat_name = update.chat.title().unwrap_or("Unknown");
    let user = &update.new_chat_member.user;

    // Check if user joined or is still a member (not left/kicked/banned)
    let is_member = matches!(
        update.new_chat_member.kind,
        ChatMemberKind::Member(_)
            | ChatMemberKind::Administrator(_)
            | ChatMemberKind::Owner(_)
            | ChatMemberKind::Restricted(_)
    );

    if is_member && !user.is_bot {
        let conn = db.lock().await;
        let _ = db::upsert_user(&conn, chat_id, user.id.0 as i64, &user.first_name);
        log::info!(
            "[{}] Member update - joined/updated: {} (ID: {})",
            chat_name,
            user.first_name,
            user.id.0
        );
    } else if !is_member {
        // User left or was removed - delete from database
        let conn = db.lock().await;
        let _ = db::delete_user(&conn, chat_id, user.id.0 as i64);
        log::info!(
            "[{}] Member update - left/removed: {} (ID: {})",
            chat_name,
            user.first_name,
            user.id.0
        );
    }

    Ok(())
}

/// Tracks user from a message and logs it
async fn track_message_user(msg: &Message, db: &Db) {
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return;
    }

    let chat_name = msg.chat.title().unwrap_or("Unknown");

    if let Some(user) = &msg.from {
        if !user.is_bot {
            let conn = db.lock().await;
            let _ = db::upsert_user(&conn, msg.chat.id.0, user.id.0 as i64, &user.first_name);
            log::info!(
                "[{}] Tracked user from message: {} (ID: {})",
                chat_name,
                user.first_name,
                user.id.0
            );
        }
    }
}

/// Handles regular messages, tracks users and handles join/leave events
async fn message_handler(_bot: Bot, msg: Message, db: Db) -> ResponseResult<()> {
    track_message_user(&msg, &db).await;

    // Only process join/leave in groups/supergroups
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        return Ok(());
    }

    let chat_name = msg.chat.title().unwrap_or("Unknown");
    let conn = db.lock().await;

    // Track new members that joined (from the message's new_chat_members field)
    if let Some(new_members) = msg.new_chat_members() {
        for user in new_members {
            if !user.is_bot {
                let _ = db::upsert_user(&conn, msg.chat.id.0, user.id.0 as i64, &user.first_name);
                log::info!(
                    "[{}] New member joined: {} (ID: {})",
                    chat_name,
                    user.first_name,
                    user.id.0
                );
            }
        }
    }

    // Track if someone left (from the message's left_chat_member field)
    if let Some(user) = msg.left_chat_member() {
        let _ = db::delete_user(&conn, msg.chat.id.0, user.id.0 as i64);
        log::info!(
            "[{}] Member left: {} (ID: {})",
            chat_name,
            user.first_name,
            user.id.0
        );
    }

    Ok(())
}

/// Handles the /all command - tags all tracked users (admin only)
async fn command_handler(bot: Bot, msg: Message, cmd: Command, db: Db) -> ResponseResult<()> {
    track_message_user(&msg, &db).await;

    match cmd {
        Command::All(text) => handle_all_command(bot, msg, text, db).await,
    }
}

async fn handle_all_command(bot: Bot, msg: Message, text: String, db: Db) -> ResponseResult<()> {
    let chat_name = msg.chat.title().unwrap_or("Unknown");

    // Only works in groups/supergroups
    if !msg.chat.is_group() && !msg.chat.is_supergroup() {
        log::debug!("Command /all used outside of group, ignoring");
        bot.send_message(msg.chat.id, "This command only works in groups.")
            .await?;
        return Ok(());
    }

    let user = match &msg.from {
        Some(u) => u,
        None => return Ok(()),
    };

    log::info!(
        "[{}] /all command invoked by {} (ID: {})",
        chat_name,
        user.first_name,
        user.id.0
    );

    // Check if user is admin
    let member = bot.get_chat_member(msg.chat.id, user.id).await?;
    let is_admin = matches!(
        member.kind,
        ChatMemberKind::Administrator(_) | ChatMemberKind::Owner(_)
    );

    if !is_admin {
        log::warn!(
            "[{}] Non-admin {} (ID: {}) attempted to use /all",
            chat_name,
            user.first_name,
            user.id.0
        );
        bot.send_message(msg.chat.id, "Only admins can use this command.")
            .reply_parameters(ReplyParameters::new(msg.id))
            .await?;
        return Ok(());
    }

    // Fetch all administrators and add them to the database
    // This ensures we at least have all admins tracked
    if let Ok(admins) = bot.get_chat_administrators(msg.chat.id).await {
        let conn = db.lock().await;
        let mut admin_count = 0;
        for admin in admins {
            if !admin.user.is_bot {
                let _ = db::upsert_user(
                    &conn,
                    msg.chat.id.0,
                    admin.user.id.0 as i64,
                    &admin.user.first_name,
                );
                admin_count += 1;
            }
        }
        log::info!("[{}] Synced {} admins to database", chat_name, admin_count);
    }

    // Get all tracked users for this chat
    let users = {
        let conn = db.lock().await;
        db::get_users_for_chat(&conn, msg.chat.id.0).unwrap_or_default()
    };

    if users.is_empty() {
        log::warn!("[{}] No users tracked yet", chat_name);
        bot.send_message(
            msg.chat.id,
            "No users tracked yet. Users will be tracked as they send messages or join the group.",
        )
        .reply_parameters(ReplyParameters::new(msg.id))
        .await?;
        return Ok(());
    }

    log::info!(
        "[{}] Tagging {} users{}",
        chat_name,
        users.len(),
        if text.trim().is_empty() {
            String::new()
        } else {
            format!(" with message: {}", text.trim())
        }
    );

    // Build mentions using tg://user?id= links (works for all users)
    let mentions: Vec<String> = users
        .iter()
        .map(|u| {
            let escaped_name = escape_markdown_v2(&u.first_name);
            format!("[{}](tg://user?id={})", escaped_name, u.user_id)
        })
        .collect();

    let mentions_str = mentions.join(" ");

    // Build the reply message
    let reply = if text.trim().is_empty() {
        format!("||{}||", mentions_str)
    } else {
        let escaped_text = escape_markdown_v2(text.trim());
        format!("{}\n||{}||", escaped_text, mentions_str)
    };

    bot.send_message(msg.chat.id, reply)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_parameters(ReplyParameters::new(msg.id))
        .await?;

    log::info!("[{}] Successfully sent tag message", chat_name);

    Ok(())
}

/// Escapes special characters for MarkdownV2 parsing
fn escape_markdown_v2(text: &str) -> String {
    let special_chars = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut result = String::with_capacity(text.len() * 2);

    for c in text.chars() {
        if special_chars.contains(&c) {
            result.push('\\');
        }
        result.push(c);
    }

    result
}
