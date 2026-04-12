Goal: Evaluate fs123 as a replacement for internal file reading API

Rationale:

Maintaining an API for accessing files read-only across MosaicFS-enabled devices is a technical burden.
It may be better to remove this feature, and rely on the fs123 system instead. fs123 is a client/server
filesystem that works over HTTP, and it includes a FUSE filesystem. From an architectural standpoint,
it may be cleaner to have a two-layer system of fs123+mosaicfs instead of having mosaicfs handle both
file serving and it's main purpose of aggregating multiple filesystems together.

Inputs:

In addition to the inventory, refer to files in the fs123-copy directory for details about fs123.

Deliverables:

- Write a technical_details.md file in this directory with the technical details of using fs123,
  along with a list of pros and cons of this approach. In this document, include any features
  that fs123 does not currently have that would be useful for mosaicfs.

Reminders:

- Do not write any code. This is effort is for an architecture change evaluation only.
