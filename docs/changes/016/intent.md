# Intent of this proposal

## TL;DR

Most MosaicFS data is naturally sharded by node, so we don't actually need multi-master conflict
resolution for the bulk of it. Once the Tauri app shipped, PouchDB was out of the picture too —
removing the other major reason CouchDB was chosen. Discovering CouchDB isn't packaged on Debian
was the prompt to reconsider, but the architectural fit was already weak.

This proposal switches from CouchDB to SQLite and implements a custom peer-to-peer sync protocol.
The file index replicates via an append-only intent log (no conflict resolution needed — each record is owned by one node). The small amount of shared configuration uses a single config-leader node
approach (one node owns the config and other nodes' UIs send config edits to the leader).

## Background info

Today I expanded my personal MosaicFS cluster by adding the NAS. I tried to do an
"apt install couchdb" and discovered that CouchDB was not available. Looking into why,
it turns out that nobody was willing to maintain the package in the main Linux distributions
that I assume are the most popular foundation layers (Fedora and Debian). That caused
me to think "Hmm, maybe CouchDB isn't a very popular choice for a database" which led me
to ask the question of whether we should continue using it.

Some of the reasons I selected CouchDB originally:
- Multimaster replication with conflict resolution via MVCC
- Availability of PouchDB as an in-browser database that could accellerate the web UI.

I recently finished working on the Tauri app that is used as the desktop browser,
and feel like this is a more viable approach than using the web interface. The key
reason for this is that the files are already mounted on all nodes, so the challenge
is really just to open the file using the preferred file handler. Doing this from a
web browser is hard, so now the Tauri app takes care of resolving the virtual path
to a real mountpoint on the local machine and opening the file. PouchDB
doesn't seem like a useful part of the architecture, and the plan was to add Redb
instead as a low-level cache for the API server to speed up responses and provide
offline browsing/search capability.

Then it occurred to me that most of the data in the MosaicFS database is naturally
sharded per node, so we don't really operate as a multi-master database; instead we
are more of a aggregation of sharded data into a single queryable read-only table. 
Each node writes to its set of data, and those writes need to be replicated to all
nodes, but two nodes will never try to write authoritatively to the same file. 

That leaves the small subset of configuration items like labels, rules, replication
rules, mount rules, etc. If those could be handled by some (hand waving) other way,
then we could avoid the need to bring in CouchDB. Maybe SQLite would work?

## Goals

What I hope to gain by switching to SQLite:

- Easier to package and deploy. Mosaicfs is built as a single artifact containing the
database (embedded SQLite), agent, server, and web UI. No need to install a separate
database. Distributions and OS vendors typically provide SQLite, so this could be used as an
alternative to the embedded version.
- Easier to test. Using an embedded database makes writing tests easy; you can just spin up an in-memory database, run the tests, and destroy the object.
- Easier to administer. The end user does not need to think about databases at all. The database
is just a thing inside of mosaicfs that takes care of their data. Replication is not some
thing to be configured via CouchDB; it can be handled automatically inside the MosaicFS codebase.
- Could provide full text search. SQLite has an FTS5 module that provides full-text search functionality. This could be a valuable thing to have to enrich the MosaicFS search experience.
- Provides an upgrade path to "real database servers". Starting out with SQLite makes it easier
to support an RDBMS like PostgreSQL in the future. There are many hosted database options from
various cloud providers that support SQL. Switching to an SQL-based database now makes it
easier to migrate to other database providers in the future.

What I don't like about going down this path:

- The replication and syncing protocol is now our responsibility. If we get something wrong,
having to blow away one node's database and resync everything is a reasonable solution. 
