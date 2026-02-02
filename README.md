# telegram_tagger

A simple self hostable telegram bot that allows you to tag all the members inside of a group chat

## Disclaimer
It appers that telegram bots have some limitations, they can't fetch the members of the group whenever they want so this bot relies on the events triggered when a user joins the group or send a message. To make it short, this bot is **unable** to tag users that were present in the group chat before his introduction unless they send a message while the bot is active!

## How to use

Any admin can send a message that start with `/all`, the bot will reply to the message with the same content of the original one but appending at the end a list of all users tagging them

## How to run

Create a `.env` file containing the bot token

```env
TELOXIDE_TOKEN=1111111111:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
```

Build and run the bot
```bash
cargo build --release
```

```bash
./target/release/telegram_tagger
# Add a .exe at the end of the file if you are using windows (don't use windows please)
```