# Intent of this change

I want to deploy the MosaicFS agent to my Linux NAS. Originally I thought about running in a container,
but now I want to run it directly on the host OS as a systemd service. Here are my reasons for
making the change:

* The desktop app was adapted to use the MacOS App Sandbox, and I feel that offers a strong
level of protection in an elegant way. I'm hoping the Linux hardening effort will be similar.
* Using a dedicated system account (mosaicfs:mosaicfs) makes it easier to manage file permissions
to limit what is exposed to mosaicfs. This allows ACLs and POSIX MAC to be used.
* If used correctly, Landlock, seccomp, and eBPF can be more secure than using Docker/Podman.
* This project is for personal education and exploration. Containers are boring and well understood,
while it is exciting to learn about Linux security mechanism.
* If we go down this path and get stuck when we are 90% of the way there, we can always give
up and go with containers for the final extra bit of security.
* The ability to run on the host OS does not preclude running in a container later. If this project
becomes widely used, some people may prefer the simplicity of using a container.

To do this safely, I want to use a layered approach that combines various Linux security technologies.

For details, see:

	./mosaicfs_linux_security_summary.md

There are some unanswered questions that we should explore before implementing:

1. Is this actually a good idea, or is using a container better?
2. Is the proposed solution in the mosaicfs_linux_security_summary.md accurate? It was produced
by Haiku which is not the deepest-thinking model.
3. What are the risks of this failing? Have people reported success with this approach, or
are there known issues and hidden complexities that will create problems?
