# MosaicFS

*A Unified Filesystem of Filesystems*

**Architecture & Design Document — v0.1 Draft**

---

## Table of Contents

**Part One — High-Level Architecture**
- [Problem Statement](#problem-statement)
- [High-Level Architecture](architecture/01-high-level.md)
  - [Sample Deployment](architecture/01-high-level.md#sample-deployment)
  - [Core Components](architecture/01-high-level.md#core-components)
  - [Client Applications](architecture/01-high-level.md#client-applications)
  - [Data Flow](architecture/01-high-level.md#data-flow)
  - [The Virtual Filesystem Namespace](architecture/01-high-level.md#the-virtual-filesystem-namespace)
- [Design Decisions](architecture/02-design-decisions.md)
- [Security](architecture/03-security.md)
  - [Threat Model](architecture/03-security.md#threat-model)
  - [Trust Boundaries](architecture/03-security.md#trust-boundaries)
  - [Secret Storage at Rest](architecture/03-security.md#secret-storage-at-rest)
  - [Network Exposure](architecture/03-security.md#network-exposure)
- [Federation](architecture/04-federation.md)
  - [The Sovereignty Model](architecture/04-federation.md#the-sovereignty-model)
  - [Export Modes](architecture/04-federation.md#export-modes)
  - [Cross-Instance Authentication](architecture/04-federation.md#cross-instance-authentication)
  - [v1 Accommodations](architecture/04-federation.md#v1-accommodations)

**Part Two — Technical Reference**
- [Technology Stack & Data Model](architecture/05-data-model.md)
  - [Document Types at a Glance](architecture/05-data-model.md#document-types-at-a-glance)
  - [Replication Topology](architecture/05-data-model.md#replication-topology)
  - [CouchDB Indexes](architecture/05-data-model.md#couchdb-indexes)
- [CouchDB Document Schemas](architecture/06-document-schemas.md)
  - [File Document](architecture/06-document-schemas.md#file-document)
  - [Virtual Directory Document](architecture/06-document-schemas.md#virtual-directory-document)
  - [Node Document](architecture/06-document-schemas.md#node-document)
  - [Credential Document](architecture/06-document-schemas.md#credential-document)
  - [Label Rule Document](architecture/06-document-schemas.md#label-rule-document)
  - [Plugin Document](architecture/06-document-schemas.md#plugin-document)
  - [Replication Rule Document](architecture/06-document-schemas.md#replication-rule-document)
- [FUSE Inode Space & VFS Access](architecture/07-vfs-access.md)
- [Authentication](architecture/08-authentication.md)
- [REST API Reference](architecture/09-rest-api.md)
- [Backup and Restore](architecture/10-backup.md)
- [Agent Crawl and Watch Strategy](architecture/11-agent-crawl.md)
- [Rule Evaluation Engine](architecture/12-rule-engine.md)
- [Plugin System](architecture/13-plugins.md)
  - [Plugin Runner Architecture](architecture/13-plugins.md#plugin-runner-architecture)
  - [Plugin Security Model](architecture/13-plugins.md#plugin-security-model)
- [Search](architecture/14-search.md)
- [VFS File Cache](architecture/15-vfs-cache.md)
- [Deployment](architecture/16-deployment.md)
- [Observability](architecture/17-observability.md)
- [Storage Backends](architecture/18-storage-backends.md)
- [Web Interface](architecture/19-web-ui.md)
- [Open Questions](architecture/20-open-questions.md)

---

## Problem Statement

Modern power users accumulate data across laptops, desktops, NAS devices, virtual machines, and multiple cloud services. No single tool provides a unified view of all that data or a consistent way to access it. MosaicFS solves this with a peer-to-peer mesh of agents that index every file in every location, a central control plane that aggregates that knowledge, and a virtual filesystem layer that presents everything as a single coherent tree — accessible from any device, to any application that can open a file.

**Target scale ("home-deployment scale").** MosaicFS is designed for a single power user managing their personal data. The architecture assumes and is tested against the following scale envelope:

| Dimension | Target | Notes |
|---|---|---|
| Indexed files | Up to 500,000 | Total across all nodes. CouchDB indexes and rule engine evaluation are designed for this range. |
| Nodes | Up to 20 | Agents and storage backends combined. |
| Virtual directories | Up to 500 | Including nested children. |
| Mount sources per directory | Up to 20 | More is possible but readdir latency grows linearly with mount count. |
| Label rules | Up to 200 per node | Effective label computation is O(rules) per file. |
| Concurrent VFS users | 1–3 | The FUSE mount is single-user; multiple applications on the same machine is fine. |
| Plugin configurations | Up to 10 per node | Each plugin adds a worker pool; too many compete for CPU and I/O. |
| Browser sessions | Up to 5 | PouchDB replication multiplied across many sessions increases CouchDB load. |

Performance beyond these limits is not guaranteed but is not expected to fail catastrophically — degradation is gradual (slower readdir, longer replication sync times, higher CouchDB CPU usage). The phrase "home-deployment scale" used throughout this document refers to this envelope.

---
