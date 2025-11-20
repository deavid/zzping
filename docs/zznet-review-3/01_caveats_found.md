# List of caveats and problems found so far

## src/net/zznet-transport-tcp/src/framing.rs

read_frame: Could be abused by an attacker or broken client, by opening too many connections and sending partial frames.

We limited the amount of memory allocation, however still the DOS vector is possible by sending 15MB for a 16MB frame
and keep doing this by reopening more connections and leave them zombie.

There are two ways that we should consider mitigating:

* A read timeout for mid-frames only. A 30 second could be quite convenient, or 300s to be sure. But this should be
  probably configurable. See TransportConnection config.

* Limit the amount of open connections that a certificate can have at any time, and force close the old ones when the
  limit is exceeded. This would effectively limit the ability to DOS with just one certificate - but we need to be wary
  because we expect the regular installs to have a collector root certificate to be copied to several machines. We could
  for example have a limit of 32 connections per certificate, that would work on most scenarios. Alternatively we could
  consider adding a parameter to the certificate to define this in a per-certificate level, or do it simply per role, or
  exception per user (i.e. 128 connections for root, 8 connections for everyone else).