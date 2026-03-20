'use strict';

const { Client, GatewayIntentBits } = require('discord.js');
const { Database } = require('spectre.db');

const client = new Client({
  intents: [GatewayIntentBits.Guilds, GatewayIntentBits.GuildMessages, GatewayIntentBits.MessageContent],
});

const db = new Database('./src/data/database', {
  cache:    true,
  autoSave: 5000,
  backup:   true,
});

client.db = db;

db.ready.then(() => {
  console.log('[DB] Ready');
  client.login(process.env.TOKEN);
});

client.once('ready', () => {
  console.log(`[Bot] Logged in as ${client.user.tag}`);
});

client.on('messageCreate', async (message) => {
  if (message.author.bot) return;

  const prefix = '!';
  if (!message.content.startsWith(prefix)) return;

  const args    = message.content.slice(prefix.length).trim().split(/\s+/);
  const command = args.shift().toLowerCase();

  if (command === 'coins') {
    const userId = message.author.id;
    const coins  = client.db.get(`users.${userId}.coins`) ?? 0;
    return message.reply(`You have **${coins}** coins.`);
  }

  if (command === 'daily') {
    const userId  = message.author.id;
    const lastKey = `users.${userId}.lastDaily`;
    const last    = client.db.get(lastKey) ?? 0;
    const now     = Date.now();
    const cooldown = 24 * 60 * 60 * 1000;

    if (now - last < cooldown) {
      const remaining = Math.ceil((cooldown - (now - last)) / 3600000);
      return message.reply(`Come back in ${remaining}h for your daily coins.`);
    }

    await client.db.transaction(async (tx) => {
      const current = tx.get(`users.${userId}.coins`) ?? 0;
      tx.set(`users.${userId}.coins`,     current + 100);
      tx.set(`users.${userId}.lastDaily`, now);
    });

    return message.reply('You claimed your **100** daily coins!');
  }

  if (command === 'give') {
    const target = message.mentions.users.first();
    const amount = parseInt(args[1], 10);

    if (!target || isNaN(amount) || amount <= 0) {
      return message.reply('Usage: `!give @user <amount>`');
    }

    const senderId = message.author.id;
    const balance  = client.db.get(`users.${senderId}.coins`) ?? 0;

    if (balance < amount) {
      return message.reply(`You only have **${balance}** coins.`);
    }

    await client.db.transaction(async (tx) => {
      const senderCoins   = tx.get(`users.${senderId}.coins`)  ?? 0;
      const receiverCoins = tx.get(`users.${target.id}.coins`) ?? 0;
      tx.set(`users.${senderId}.coins`,   senderCoins   - amount);
      tx.set(`users.${target.id}.coins`,  receiverCoins + amount);
    });

    return message.reply(`Sent **${amount}** coins to ${target.username}.`);
  }

  if (command === 'leaderboard') {
    const { data } = client.db.paginate('users.', 1, 10, 'data', true);
    const filtered = data.filter(({ ID }) => ID.endsWith('.coins'));
    const lines    = filtered.map(({ ID, data: coins }, i) => {
      const uid = ID.split('.')[1];
      return `${i + 1}. <@${uid}> — ${coins} coins`;
    });
    return message.reply(lines.length ? lines.join('\n') : 'No data yet.');
  }
});

process.on('SIGINT', async () => {
  console.log('[Bot] Shutting down...');
  await client.db.close();
  process.exit(0);
});