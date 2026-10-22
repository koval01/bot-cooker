# How it works

## The persona

Every instance has a secret **master key**. From it, bot-cooker derives a
*persona*: an imaginary organisation and the system it runs.

- identity: company name, industry, domain, staff, locale and time zone;
- technology: one of ten application stacks (Laravel, Symfony, Django, Rails,
  Express, Spring Boot, ASP.NET Core, WordPress, Joomla, Drupal), a front
  server (nginx, Apache, OpenResty, Caddy, IIS, LiteSpeed) that fits the
  stack and the host's operating system, real released versions of each, a
  database, a cache, a mail provider, a cloud;
- habits: how its configuration files are quoted and indented, which
  comments and leftovers they contain, how its logs are formatted;
- behaviour: which paths exist, which leaks lie around (`.env`, `.git`,
  backups, logs, `phpinfo`), header order, how fast it answers.

Each facet comes from its own random stream keyed by the master key, so the
persona is stable for that key and unrelated to every other key's.

## Deterministic content

Every response is a pure function of the master key, the requested path and
the current **epoch**. Asking for the same file twice returns the same bytes,
as a real file would. Epochs then move the content forward the way a
maintained system changes:

- configuration epochs (about a week by default) rotate secrets and bump
  patch versions;
- log epochs (a few hours) extend logs with new lines;
- epoch boundaries are jittered per instance, so instances do not change in
  lockstep.

Nothing is stored: no database, no files written. A restart with the same key
serves the same system.

## What scanners find

- **Leaked configuration** in the stack's own format: `.env`,
  `wp-config.php`, `configuration.php`, `settings.php`, `database.yml`,
  `application.yml`, `appsettings.json`, `docker-compose.yml`. Values agree
  with each other and with the rest of the persona.
- **Version control**: `.git/HEAD`, `config`, `logs/HEAD`, refs, pointing at
  a plausible remote.
- **Database dumps** in the right tool's format (mysqldump, MariaDB,
  pg_dump, SQL Server) with the stack's real schema and consistent records.
- **Logs**: access logs in the server's format, error logs, application logs,
  in chronological order, with the background noise every public server has.
- **APIs** shaped like the stack's own, with real pagination envelopes.
- **Pages**: login screens, `phpinfo`, `server-status`, version files,
  sitemaps, feeds, and a website whose pages and directory listings lead
  deeper — the labyrinth. Pages link stylesheets and scripts that really load.
- **Realistic misconfiguration**: some PHP personas print PHP errors into
  their pages, consistent with their own configuration and logs.

`robots.txt` lists, as "disallowed", paths that really exist on this
instance: well-behaved crawlers stay out, recon tools go in.

## How it answers

- **Server behaviour**: paths the persona does not have get that stack's or
  that server's real 404; denied dotfiles get 403; directories redirect to
  their trailing slash; `HEAD`, `OPTIONS`, unknown methods and malformed
  requests are answered the way the persona's server answers them, down to
  header order and spelling.
- **Pacing**: headers arrive after the application's usual response time — a
  few hundred milliseconds, as from a PHP or Rails backend — so the server
  looks alive. The time is spent in the body, which trickles out in pieces
  whose size and spacing change during the response (brisk stretches,
  sluggish ones, stalls, network hiccups followed by a burst), never silent
  long enough for a client's read timeout to give up. Some connections are
  cut at different stages; some requests are answered at normal speed, as a
  cache hit would be. All distributions are per instance.
- **Static files** (stylesheets, scripts, `favicon.ico`, `robots.txt`) are
  answered at once, as a web server serves files from disk.
- **Limits**: per-connection byte and time caps, and a global cap on held
  connections that protects the host: past it, new requests get fast answers
  instead of another held slot.

## What it never does

It never opens an outbound connection, never runs anything a client sends,
never writes to disk, and never sends content meant to harm a client. See
[Ethics and responsible use](ethics.md).
