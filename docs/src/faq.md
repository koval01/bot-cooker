# FAQ

## Is this legal?

Tarpits and honeypots on your own infrastructure are a long-established
defensive technique: you decide how your own server answers requests sent to
it. bot-cooker sends nothing to anyone who did not connect to it, and sends
nothing harmful. Rules still differ by country and by hosting provider, so
check your provider's terms. This is not legal advice.

## Will it hurt real users or search engines?

Not on a dedicated hostname: no person or legitimate crawler has a reason to
go there. `robots.txt` asks crawlers to stay out of the decoy paths, and
well-behaved ones do. Do not put the trap on a hostname your users rely on.

## Why not just block scanners?

Blocking is free for the scanner: it moves on in milliseconds. A tarpit makes
each attempt cost minutes and returns nothing usable. Use both: block what
targets your real services, and let traps raise the cost of indiscriminate
scanning.

## Can scanners tell it is a trap?

Making that hard is most of the work in this project: personas, real server
behaviour, real versions, consistent artifacts, per-instance timing and an
automated audit that looks for shared lines across instances. No disguise is
perfect; if you find a reliable tell, please report it privately (see the
security policy) so it can be fixed.

## How many resources does it need?

Very few. Each held connection costs about 30 KiB of memory and one open
file; the default cap of 8000 connections stays around 250 MiB. A small VPS
is plenty.

## Does it store anything?

No. Decoy content is generated on the fly from the master key; counters live
in memory. The only thing written is the log, where you configure one.

## Why Rust and no web framework?

A trap must speak HTTP the way the server it imitates does, down to header
order, spelling and error pages, which general-purpose frameworks normalise
away. A small wire layer of its own gives that control and keeps the binary
small.

## Can I use it at work, in a company or a public institution?

Yes. The Apache 2.0 license allows any use. Keep the `NOTICE` file when you
redistribute it or a modified version.
