# Core utilities

Status: PLANNED. META uses FreeBSD's base utilities. GNU versions are available from
packages under names starting with `g`, and Linux programs see the utilities of their own
root.

A Rust utility belongs here only if it does something the base does not. Rewriting existing
tools is not a goal. Nothing replaces a base utility in place, because base packages own
those files and scripts depend on their behaviour.
