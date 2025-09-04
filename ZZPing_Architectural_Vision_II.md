# ZZPing Architectural Vision II

## *Concept for v0.3*

**Author:** [David Martínez Martí](mailto:deavidsedice@gmail.com)

**Created Date:** Aug 16, 2025

**Last major update:** Aug 22, 2025

**zzping version:** 0.2.2-beta2, concept for v0.3

To-do: consider multiple gateways, i.e. backup internet with two ISP, collector having two source IP addresses.

This document lays out the high-level architectural vision for the next major iteration of zzping, version 0.3.

IMPORTANT: This document is just an overall direction for the project, to identify the core challenges and explore potential solutions. Details are intentionally left open.

# Rationale

There are plenty of ping tools available already, but a tool that can actually monitor, store, and graph a network doesn’t seem to exist, at least in the FOSS space.

Picture this: Your video conference or game glitches in the worst moments. You run a ping, some packet loss. Ask the ISP and you get back: “maybe it’s your computer, did you try from another computer? Have you restarted the router? the computer?” and we can be circling on this set of excuses permanently.

Meanwhile your connection is still bad at times. You need data to prove it. You need to make sure it’s not “your computer”, and that is not the fault of whatever host you are pinging.

Sometimes the problems are so short lived that it looks like a small packet loss... but in reality it might be a “micro cut”, where you have no connection for a small fraction of a second.

How do we identify and quantify these problems? This is why **zzping** was born:

1) We should not wait until there’s a problem to start a ping. Instead, we should have constant background pings. And store this data for later analysis.  
2) Some problems are very bad but short lived. We need high frequency pings (\~100 pckt/s) in order to separate packet loss from a micro cut.  
3) The problem might lie on your computer, or in a particular host on the internet, or in the router. We need to ping several hosts simultaneously such that we can rule out all those scenarios.  
4) It might just be the CPU on the computer that is running the pings. We should be pinging from different computers at the same time, to rule out that it’s the computer itself freezing.  
5) Being able to store a year of high resolution data to be able to correlate if the problem recurs in certain months (i.e. heat waves)  
6) Having a “bad minutes” SLI for capturing overall metrics and overall health indicators.

NOTE: zzping is aimed for a regular home, with just a few computers (or just one) to inspect and debug internet problems. It is not particularly suited for offices, datacenters nor homelabs.

## Non-Goals

* **zzping is not a network discovery tool.** It will not automatically scan the network to find potential hosts. All monitoring targets must be explicitly configured by the user.  
* **zzping is not a generic metrics platform.** It is a purpose-built system designed specifically for analyzing ICMP echo request/reply data. It will not support monitoring arbitrary metrics (e.g., CPU temperature, SNMP data, web server response codes).  
* **zzping is not a security monitoring tool.** The authentication mechanisms are designed for basic access control within a trusted LAN environment, not for securing the system against a sophisticated or hostile actor.  
* **High availability is not a primary goal.** It is not intended to be a fully fault-tolerant, replicated system. The database, in a standard configuration, remains a single point of failure.

## Guiding Principles

1. **Simplicity Over Complexity.** We will always prefer a simpler, more robust solution over a complex one that offers only marginal gains. We will aggressively manage scope to avoid building features that are not essential to the core mission.  
2. **Robustness and Resilience First.** The system is designed to run continuously for years. It must be resilient to component restarts, network interruptions, and unexpected failures. Data integrity and the elimination of data gaps are paramount.   
3. **The User Owns Their Data.** The data collected by zzping belongs to the user. The system must provide clear and simple mechanisms for users to export their data into standard, interoperable formats. We will not create a locked-in data silo.   
4. **Clarity Through Observability.** A monitoring tool that cannot be monitored is untrustworthy. The system must be transparent about its own operational health, providing clear, at-a-glance feedback that allows a user to distinguish between a network failure and a system failure.   
5. **Minimal Resource Footprint.** The collector and database services are designed to be "good citizens" on a home user's machine. We will prioritize low, stable CPU and memory usage to ensure the system can run continuously without impacting the primary use of the computer.

# Scrapping the old code (v0.2.2)

The plan is to redo zzping entirely \- it is unclear if there is anything at all that would survive after this re-architecture. It seems that the project will be completely rewritten.

The old architecture design is a zzping-daemon that performs the pings and stores to disk, and a zzping-gui that reads from disk. The user needs to run a tool to transform the files from zzping-daemon into a new more compact format that zzping-gui can read. The GUI can show and navigate through billions of points at different resolutions. 

But there were plenty of problems. The real-time part was never fleshed out, restarts created data gaps, the disk usage was quite high.

A simpler single GUI program that does everything would not suit the needs I’m trying to address, which requires a service.

I attempted to store just statistics instead of the full pings, to reduce disk usage and make visualization easier. The problem I found with this is that as soon as I wanted to try a slightly different statistic, I lost all the history of pings.

I also wanted to perform pings from a second machine too, because that gives important data to discard that a single machine is the problem, and that needs a better architecture.

Therefore we will scrap the current model, the FrameDataQ and other methods of compression, and start from scratch again. Also please note that FFT and DCT are meant to be compression attempts that did not yield the expected results; we might try again later, but they’re out of scope for v0.3

# Desired Architecture

zzping will be divided in three main programs categories:

* zzping-database: This is a background service that will be storing all the information to disk. It exposes a TCP/IP server socket where all other programs will connect.  
* zzping-collector: This is a background service in charge of actually performing the pings and sends them to the database.  
* zzping-gui / zzping-cli: User applications that connect to the database to query, and to set configuration.

The typical deployment looks like this:

* Machines that will be performing pings: they need zzping-collector service installed.  
* Central machine that will be almost always powered on, with plenty of disk space to store the historical data: will have the zzping-database service.  
* End user computers: will have zzping-gui to see the state of the network.

Of course, the most simple and typical setup is just a single computer that hosts everything.

The database acts as the central server, every application connects to it.

The configuration for the apps is minimal \- all relevant configuration is fed by the database. This means that the collector and GUI only have to store locally how to connect to the database.

The connection between apps will be plain TCP/IP, no SSL. A simple authentication will be provided to prevent potential abuse by actors that manage to reach the TCP port of the database. Since this is aimed to be LAN only and the data that it transfers isn’t particularly sensitive, SSL/TLS should not be needed \- therefore there are no plans to implement it.

## Collector Service

The collector will have the ping functionality implemented as a swappable backend, isolated from the core collection logic. This provides the architectural flexibility to introduce alternative backends in the future, such as a traditional raw socket pinger for environments where unprivileged pings are not feasible, or other diagnostic protocols (e.g., UDP, TCP) if the need arises.

We can also use this swappable backend to have a fake-ping backend that could be used for unit testing, if it turns out that doing unit testing is doable and gives value.

Upon start, the collector will attempt to connect to the database. Then it will receive the config from the database on who to ping and at what frequency, and it will do so. It will transmit back to the database the result of the pings as they happen, in real time.

It will store the current state of all ping operations in-memory and will buffer the resulting data for a configurable duration (e.g., the last X hours). This buffer ensures data continuity across database disconnections or restarts. In case of a connection error, the collector will attempt to reconnect to the database automatically every 2 seconds. Once reconnected, it will first stream the historical data from its in-memory buffer to backfill any gaps before resuming the live data stream.

The collector is a "dumb worker"; it receives its entire configuration (ping targets, frequencies, etc.) from the database upon connection. It must also support receiving live configuration updates from the database during its operation.

In the event of a prolonged disconnection from the database, the collector will enter a "fail-static" mode for a configurable period (e.g., 24 hours), continuing to ping with the last known configuration while buffering the data. If it cannot reconnect within this period, it will transition to a "fail-close" mode to reduce CPU and network usage when it’s not needed. In this mode, it will stop initiating new pings and will gracefully discard old data from its buffer as new data arrives, effectively maintaining only the most recent X hours of data until a connection can be re-established.

In addition to streaming ping data, the collector will periodically report its own operational health metadata to the database. This includes its connection state, in-memory buffer usage, and any critical errors encountered (e.g., failure to create ICMP sockets).

## Database Service

This would be the central hub, server, that every other service or app connects to, the main brain of operations.

Its responsibilities are:

* TCP/IP server socket for collectors and applications to connect  
* Receiving collectors ping data, storing it on memory, and regularly compacting and flushing to disk  
* Performing offline aggregation of data, to obtain overall statistics over a minute, an hour, a day.  
* Deleting old data when it exceeds a size threshold.  
* Serving real-time ping data to applications that subscribe to this.  
* Performing read queries for apps (GUI) over the aggregated data, or the individual pings  
* Maintaining indexes to speed up queries (where each time range is on disk)  
* Sending dynamic configuration data to the collectors. Receiving configuration changes from the GUI/CLI apps.  
* Orchestrating the collectors together to get the desired amount of pings to hosts.  
* Receiving and tracking the health status of all connected collectors.   
* Serving historical data queries from client applications. The database will provide an efficient API for retrieving raw and aggregated ping data for specific time ranges, serving it in a single, canonical binary format.

Failure Modes and Reliability:

* As the stateful core of the system, the database is designed for data integrity. It is the only component that writes persistent state to disk.  
* While it is a single point of failure in the standard configuration, its restart is non-destructive. Upon restarting, it will automatically be repopulated with recent data from the in-memory buffers of the connected collectors, minimizing data gaps.  
* No replication will be available for the database, meaning that is the single point of failure of zzping. It might be possible to design a master-master replication in the future, but given this software is aimed for home setups, it seems overkill to do so.

## GUI and CLI apps

The end-user applications will be used for:

* Provide awareness of network health through a **system tray monitor**. This monitor will offer at-a-glance real-time data and will visually indicate when new "Bad Minute" events are occurring (e.g., by changing color or blinking). It will also serve as a persistent indicator of unseen historical events, allowing the user to be passively aware of issues without requiring intrusive notifications.  
* **Graph past data**, allowing for rapid navigation through millions of data points, and provide high-level dashboards for long-term reliability analysis. This includes views for reliability scores (e.g., "% of good minutes") and trends of "bad minutes" over days, weeks, or months.  
* **Configure the settings live**, increase pinging, add new hosts, etc.  
* Consuming the **system health** API from the database to provide the user with a clear, at-a-glance view of the entire zzping system's status. This is crucial for diagnosing issues and differentiating between a network outage and a problem with a zzping component.  
* Handling all data transformation and formatting. The client applications will be responsible for parsing the canonical binary data provided by the database and transforming it into user-facing formats. This includes both rendering data for on-screen graphs and providing a user interface for exporting data into standard formats like CSV. This design keeps the database's role focused solely on efficient storage and retrieval, and any code duplication between the GUI and CLI can be mitigated by using shared crates within the zzping workspace.

The CLI would be a simple command line to do basic stuff, basic queries, setting configs, etc.

The GUI would be the main target of all the development.

# Challenges

* **Sending ICMP:** Typically requires a raw socket, which requires root/admin, usually via \+setuid or \+setguid. This is a security problem. Instead we will try unprivileged ICMP.  
* **Near Zero CPU consumption:** we want this to run permanently, if it uses any CPU resources, it takes away from other uses of the computer. This is for a home, not a server.  
* **Storing data to disk:** Efficiently storing billions of data points across years to build history. If not done carefully it would require several TiB of space.  
* **Indexing:** This data needs to be queried, and we need ways for fetching the data ranges and src-dst pairs in an efficient manner for quick graphing.  
* **Offline aggregation:** The data often needs to be seen in a very zoomed out way to look for what we need. We should not do live aggregation, because processing several million data points is not trivial.  
* **Zero downtime on updates:** We don’t want data gaps when updating the app, or other small outages.  
* **UDP as heartbeat:** We can use UDP to quickly detect when the peer has disconnected. We can also use UDP broadcast to discover the database IP.  
* **Dynamic Configuration:** The system must be easy to manage and tune on the fly. Configuration needs to be centralized, editable from a single interface (the GUI), and propagated to all components (the collectors) without requiring restarts or causing data gaps. This requires a robust model that separates the user's intent from the system's live operational state.  
* **Authentication:** A basic mechanism to prevent bad actors from controlling the services that works well in clear TCP/IP.  
* **GUI for showing million data points:** Most GUI frameworks are not prepared to show so many points so fast.  
* **Simple Installation and Deployment:** A system with 2 different services and such could be very cumbersome to set up for new users \- it needs to be simplified.  
* **Defining "Bad" Performance:** Having an SLI metric for the network, that we can count “bad” minutes, could be very helpful for long term data inspection.  
* **System Health and Observability:** A monitoring system that cannot report its own status is not trustworthy. A blank graph is ambiguous: is the network down, or has the collector crashed? The system must provide clear, at-a-glance feedback on its own operational health, allowing users to differentiate between an external network failure and an internal system failure.  
* **Data Usability and Context:** How do we extract value from all the data stored? Graphing, exporting and overviews are important such that the user can obtain valuable information.

Please note that most of these are open problems \- I already laid out some potential ideas, but they are not definitive. We probably need to continue investigating each one to get to an actual decision on what to do.

## Sending ICMP

What we typically refer to as a “ping” is an ICMP echo packet. And this is a [Layer 3 protocol](https://www.cloudflare.com/learning/ddos/glossary/open-systems-interconnection-model-osi/) that assists IP in their operations. ICMP not only does “ping” but much more. We are only interested in performing pings and listening to the replies, however due to the nature of ICMP, most operating systems limit this to the root user or administrator.

And it’s not like ICMP itself is limited; the only protocols that are allowed for normal applications are TCP and UDP sockets. If anyone needs anything that it’s neither TCP or UDP, they need to go with a raw socket. A raw socket allows you to send and receive absolutely anything and can be quite dangerous.

So this means that an application performing ICMP would typically require root permissions. On Linux this is typically done with \+setuid and \+setguid permissions. The problem with this is that these are a privilege escalation by definition. You call “ping” from “myuser”, but Linux sees the setuid and starts “ping” as root. So if there’s a way for “ping” to be abused, or broken, a bad actor could craft a ping command that could escalate permissions. For example, imagine if there was a buffer overflow with code execution: the attacker could use “ping” to open a bash shell as root.

If this were the only way, I would isolate the pinger part onto the smallest binary possible, such that it can be audited. However, there seems to be two other workarounds:

Linux has “capabilities” \- handled by the “setcap” program. And we can configure a program to have “CAP\_NET\_RAW” \- this would allow the program to create raw sockets without needing root access.

This reduces the surface attack enough for me to be confident that we don’t need to separate the pinger part into a small binary. However this might not be available on other operating systems.

And there’s another, even better way: Unprivileged ICMP. Linux, Mac and Windows have this (although Windows is different on how it is implemented). It allows for very basic ICMP echo requests (pings) that suffice for what we want. It does not need the binary to be marked in any way, but it needs the OS to allow the users to do this [via sysctl](https://ekman.cx/articles/icmp_sockets/).

We will try to use Unprivileged ICMP pings using [ping-rs](https://docs.rs/ping-rs/latest/ping_rs/), since it works on Mac and Windows too. Not that I’m interested in building on these, but in case anyone in the future wants to.

This may require OS-level configuration (e.g., net.ipv4.ping\_group\_range via sysctl on Linux) to grant permissions to the user account running the collector. This requirement needs to be clearly documented to the end-user.

;

A ping is usually just an ICMP echo packet that has a sequence\_id (a number of our choice between 0 and 65535). Then we will listen and see if we hear back an ICMP echo reply that has the same sequence\_id. Measuring the time that it took between sending and replying is the RTT (Round Trip Time), which is the main data point we’re going after.

The other main data point is the lack of replies. If we never hear back, that is a packet lost. This is indicative of problems in the network and it’s very important.

**1st Proof of concept tests:**

* Had to run: sudo sysctl \-w net.ipv4.ping\_group\_range="0 2147483647"  
  * but at least it doesn’t need to be re-done every binary rebuild.  
* Ping-rs for some reason returns latency in milliseconds (integer), so we don’t know what happens between 0-1ms. Not a big deal, but sub-optimal.  
  * This seems to be a ping-rs limitation.  
* Pings were being filtered from 192.0.0.1 \- most likely VirginMedia is detecting the high ping rate and filtering ICMP. Strangely this wasn’t happening with raw sockets. Might have to do with the sequence chosen or other parameters.  
  * Happens already at 20 pings/s; also at 10 pings/s.  
  * The reason for this is that ping-rs sends all pings with seq=1; and it gets eventually filtered.

Conclusion: get rid of ping-rs. Research other libraries or implement manually. seq=1 and millisecond resolution are bad design.

surge-ping might be a tentative replacement. Other crates: ping, ping-async. More low level: tokio-ping, tokio-icmp-echo.

**2nd test zzping-collector:**

surge-ping works perfectly. Tokio works well. However, *tokio* timers and sleep seem to have an effective resolution of 1ms, which is not good enough. But we did a mixture with thread:sleep for the last 10 milliseconds in its own task spawned, and that seems to give us 20us precision, which is way more than we need.

The need for good timing precision when launching pings is that, when storing, if the pings appear in a constant cadence, we do not need to store exactly when \- we can just make an equal subdivision. Which saves us from storing a field, so 50% disk savings.

The CPU usage is below 0.2%, hard to even measure. We can ping above 200 pings per second no problem.

## Near Zero CPU consumption

Typically most networking programs make plenty of use of threads to allow for the maximum I/O throughput and precision. However, threads are not free, especially if we account for mutexes.

In zzping, at least for the services, we want to keep them as lean and efficient as possible. We want them to be running permanently, so we would aim for a consumption of less than 0.5% overall in the CPU. And we can trade off some precision for less consumption.

This is also important when it comes to “waiting”. Usually a “sleep” (or delay) isn’t guaranteed to actually sleep for exactly that amount of time. It will always sleep for a bit more, especially if the CPU is busy with other stuff \- because it takes an uncertain amount of time to schedule the thread again.

Here is where we trade off a lot of precision for efficiency. By using sleep as much as possible, the CPU doesn’t wake up, and we don’t waste resources. The typical way to handle high precision is “spinlocking” which basically means a for loop that consumes CPU cycles until we’re at the correct time.

There’s an alternative called High Precision Timers that are exactly for this. It seems that if we use tokio, the sleep function of tokio already provides this by default. So this is something to explore. Although each OS handles this differently, tokio should provide a single interface for all.

The problem is, if we sleep for too long, if we wanted to do 50 pings/s, in reality we might do 40 pings/s and underperform. This is acceptable, and we can compensate up to some degree \- but if tokio::sleep provides higher precision timers for this out of the box, and multiplatform, then we’re set.

### Staggering pings across targets and across collectors

If we do not do anything, pings might all get fired at the same time, or not, between different targets or source hosts (collectors).

I’m not sure if it’s better to stagger, but in principle, staggered pings will add additional time resolution to find problems.

For example, imagine we have 4 collectors on different machines all pinging 8.8.8.8 \- and we want the total to be 100 pings/s ; meaning that each one pings at 25 pings/s. If all of them ping at the same time, it means that the time resolution is actually 25/s not the 100/s if we stagger perfectly.

This is more of a “nice to have” than anything \- it would mean some kind of synchronization between different collectors (using the database as a means of communication). And it doesn’t need to be high precision or anything, because we expect the collectors to drift over time, so most likely we only need at most some fine adjustment; or ... even more likely, probably we do not need to consider this at all.

It could be accomplished as well by the database telling a collector to go slightly slower or slightly faster (49.95/s ... 50.05/s) temporarily to ensure the phase does not align.

## Storing data to disk

One of the most critical parts is the storage format on disk because given the speeds we aim for, we could be saving easily over a thousand events per second. If the format for storing isn’t thought through properly, it would easily fill hard drives in no time. And even if these are deleted later after X time, the strain put on hard drives \- specially SSDs \- could be important. So the objective is to store as little as possible from the get-go.

In the old history of zzping we have had lots of attempts at compression with not much success such as FrameData and FrameDataQ. We expect to scrap all of these.

One of the big problems we found in zzping v0.2.2 is declaring the pings lost. Because I was attempting to use the same data format for real-time and for storing to disk, I had to introduce a delay such that I can declare the packet lost before proceeding to save data. It is clear that we need a disk specific format, and for disk, we need to introduce a delay before saving, roughly 60 seconds. And when saving we need to sort the pings properly before saving, because they arrive out of order.  

A ping, after all, is solely expressed by the following 4 components:

* Src IP Address: The host that is emitting the ping  
* Dst IP Address: The host which is the target of the ping  
* Sent time: When it was sent  
* Option\<Recv time\>: When it was received back, if it has been received.

For the purposes of this analysis we will assume 10 targets at 100 packets/second, which is a total of 1000 pings per second. We will focus on the amount of space required per year. 1 year \= 31557600 seconds

Consider that this is an upper bound of what is sensible to do, because in reality we expect the following number of hosts:

* 2 computers on LAN (to check for the switch working properly)  
* 1 router (to check if it’s alive)  
* 3 different hosts from internet, different providers (to validate that going to internet is the problem, and it’s not a single host problem)

This list is already comprehensive, and it’s 6 hosts instead of the 10 we’re assuming. Furthermore, 100 pings per second is a bit excessive. 20 pings per second is what could be more recommendable. That means that in practice, we expect most installations to have 6\*20=120 packets per second \- instead of the 1000 we’re going with. So, just to be clear what kind of “upper bound” we’re assuming here.

With this said, the above structure would use roughly the following space:

* source\_ip: 16 bytes  
* destination\_ip: 16 bytes  
* sent\_time: 16 bytes  
* received\_time: 24 bytes

Total: 16 \+ 16 \+ 16 \+ 24 \= 72 bytes

So, for a year:

72 bytes / packet \* 100 packets / second \* 10 hosts \* 31557600 seconds in a year \= 2.07 TiBytes

It’s not terrible, but not good. This is almost dedicating a hard drive per year of data, in a home setting.

It should be possible to reduce this to slightly above 4 bits per packet with the following schema:

1) We can store each src/dst pair in different files, such that we only need to declare them in the file name and maybe the header of the file.  
2) We store sent\_time once every X minutes in full precision.  
3) For every ping, we store (u16: sent\_delta, u16: ping\_rtt) where we store the time elapsed since the last ping, and the time taken to receive the ping.

This (u16: sent\_delta, u16: ping\_rtt) will make clever use of lower precision required. That we do not need sub-millisecond precision.

For example, we could save even in 0.1ms intervals, so 1ms would equate to 10; therefore the max value of 65535 would equate to 6.55 seconds, which is long enough.

There are other formats here to be considered, such a logarithmic quantizing, where each value is maybe 0.1% higher than the previous. For example, consider the following formula:

time\_in\_ms \= (1.001^encoded\_value-1)\*100

encoded\_value \= ln(time\_in\_ms/100+1) / ln(1.001)

Using this formula, an encoded\_value of 5000 represents 14.7 seconds already. What it does is it keeps the same amount of significant digits as the number grows, with linear/lower precision on the range of 0.1ms..10ms; the problem is that it with this precision we can go up way too much (\>100y) and a value of 60 seconds is represented by 6401, so we only use 10% of the range.

This is something I haven’t settled with already, but it’s clear it is possible and that whatever we choose, roughly 4 bytes per ping is possible. 

And with 4 bytes per ping we would have 117.56 GiB in a year, which is way more manageable.

I’ve considered lots of ways to bring this further down, but going below 4 bytes per ping requires either very complex compression mechanisms that are bug prone, or heavy quantization and data loss, but most probably both. 

Delta encoding, bigger quantization steps, and [rANS encoding](https://crates.io/crates/rans) seems that, in theory, it could bring it down to 1 byte per ping. However the risks are too high \- I probably prefer to think this for archival or analyze the options after having some sample files at 4 bytes per ping. (NOTE: [constriction crate](https://crates.io/crates/constriction) looks better maintained than rANS)

It’s also unclear what’s the value of this data, at this resolution after several months, or several years. We want to do some rough aggregation of the data that should compact it several times more than what we can do here, since in this case we MUST store individual pings \- we don’t want to store statistics only at this stage. But for archival, a stats aggregation is likely enough.

Why we must store raw, individual pings: because there are infinite ways of aggregating the data, for different purposes. In my experience with zzping initial development, I found that missing the raw data basically means you lost the history when you want to tweak the aggregation.

**1st Proof of Concept test:**

We initially stored (u32,u32) from the collector service for testing, then we packed it with an ad-hoc tool to (u16,u16). Results seem very good, we get a deviation RMSE of 29 microseconds average. The size after packing is 32.96 MiB/day , or 11.76 GiB / year. This is for 1 host at 100 pings per second.

Also we managed the timing for sending pings to be within standard deviation of 40us, which means that we can easily treat it as a perfect cadence with always the same delta time between pings, and discard any jitter. That would remove the need to store timings, bringing the size to half of this, meaning that it’s perfectly reasonable to assume that a standard installation would produce less than 300 GiB after 5 years (10 hosts, 100 ping/s, not storing delta time, just rtt).

We also noted that “zstd \-9” can compress to 30% the (u16,u16) format. So it’s clear that there’s room for improvement. Remains unclear if we want to push the envelope further to try to get less than 1 byte per ping.

Also remains unclear what other techniques (delta encoding of rtt, DCT) could do to better compress the low frequency data, because on initial inspection it looks like there’s a clear continuity and low frequency that could be encoded tighter.

And no idea if the result can be then compressed further by “zstd \-9” for archival. However indexing on that is problematic.

NOTE: we probably need to start thinking about chunks and indexing.

**About ReRun**

We also tested a ReRun export, the data is 10x bigger or maybe more, so this format is not ideal for long term storage. However, it works pretty well. And thinking that ReRun in the end needs to fit the data in memory, maybe we can examine later if from the GUI/CLI we could select what to export (which hosts, from what time to what time) and do a live export via HTTP to the ReRun server. 

This could allow us to leverage ReRun for the in-depth analysis and visualization, where the GUI can stay a bit more basic. With some graphs and some minimal stuff, to basically ease of selection and export to ReRun.

Also ReRun seems to support live data too (streaming constantly) that could be interesting as well.

NOTE: ReRun already slows down with 1 plot of 8 hours at 100 pings/s because of the amount of points it has to reduce in real time. Works fast again when zooming in, but this is something that needs a solution. Managing 1 day of data would be already too much currently. ReRun can accept different streams, which we can switch at different zoom levels but that is done manually by the user. ReRun does not have an automatic zoom switching, or a way to store aggregation levels.

## Indexing

Storing the data in a super-compact manner is all well and good, but if we want to retrieve the data for March 2nd at 6am, we shouldn’t need to read the data from 6 months earlier.

Sounds obvious, but if we compress the entire files for example, because of the same nature of compression, you need to read the whole file from left to right to find what you want. You can’t skip data.

Other tight formats, even if they’re not technically a compression, might have the same issue. We need to know where to start reading the file \- where to put the cursor such that reading from there would make sense. Otherwise, if the cursor ends in the middle of a message, nothing would make sense and the program could crash or do something totally unexpected.

Therefore we need ways to quickly find the data we want, such that for a query we can say that we need an X file with the cursor at position Y. It doesn’t need to be exact, but it needs to leave us in a position where we can read properly, and it needs to be on the left (before) of the position that we really need.

It’s not about locating the exact byte location of any data, but instead, to locate somewhere close enough, such that reading from left to right from that position we can find the data in a very small amount of time. For example, reading and parsing 1 MiB of data is typically very fast in modern computers. Knowing what is at the right of each 1 MiB mark (roughly) could be enough.

The problem is, how do you store this, and where?

An option is to have “index files”, where we store metadata for each MiB roughly. But then the index has to be read from left to right \- and if possible it has to be small enough such that it can be kept in memory. This has to be accounted for.

There are techniques to allow indexes to get a very high precision, such as BTree. The problem however is that BTrees and other advanced data structures cannot be stored sequentially \- left to right. We need the ability to edit parts of the file, or rewrite it entirely. For files stored on disk, I would prefer to stick to left-to-right writing where possible, because that gives high chances of recovering correctly after a crash or a sudden loss of power. However we can also look into alternatives too, such as WAL, or having two copies and modifying one and confirming it is written before going to the other copy.

## Offline aggregation

When using the GUI to see past data, seeing just individual pings is not that useful, or just a few seconds of data. It is very common to want to zoom out to see days, months, or even a year.

The problem is that, if the GUI has to process a full month in order to display, it would be very slow to do zoom in and zoom out, or panning in a zoomed out scenario. That is, if the aggregation of data has to be done ad-hoc as it is queried. But it does not need to be.

Ideally we want to have different files at different zoom levels, for example, one where we have 1 point per second, another with 1 point per minute, 1 per hour, 1 per day... and then have statistics on them, such as percentiles (1, 25 ,50, 75, 99\) of the RTT, and packet loss for example. We will likely also need the number of packets sent, and the number of packets lost.

NOTE: That is unclear what statistics we want to store \- expect that we will need to modify these computations several times, which means that we need some mechanism to re-do all aggregation.

This should ideally happen in the database as it is processing, so when we have a full hour we could update some of these aggregated files automatically. These aggregated files should also take into account indexing, as above, because they can get a bit big, and we need to be able to find what we want quickly.

We should also have some kind of max retention, to delete things when they’re too old and use too much space. For example, the user could just set the maximum amount of space they want all files combined to use, i.e. 10 GiB. And when the total size exceeds, figure out what to delete.

Also, we need to consider CPU usage here. We probably don’t want to write to disk a few bytes at a time, so we want to wait until we have a few KiB; however, this could cause CPU spikes. To prevent this, we probably need to do continuous aggregation in memory, such that we spread the load over time, and then storing on disk is just reading from memory.

## Zero downtime on updates

One of the main reasons for going with this architecture of collector-\>database-\>gui is because I really want continuous pings to happen uninterrupted. I don’t want to lose pings and have data gaps because I closed the GUI or I had to restart the database or collector.

To make this work, we need to think of these as two different scenarios: updating the database, and updating the collector. Let’s go first with the database because it’s the easiest.

To update the database, we first replace the binary with the updated version, then we restart the database service. This will do a stop, then a start. This will ensure that the new binary is loaded. The stop+start is expected to complete in less than 10 seconds.

To avoid having a 10 second gap every time this happens, the collectors will:

1) Keep pinging even in the event of database disconnection; they will assume the last config received from the DB still stands (this is called fail-static)  
2) Keep in memory the pings of the last X hours

When the new database version finally boots up, it needs to read from disk first to see what was written so far, what is the last state; and prepare the state in memory. Once the collectors connect again, they will check with the DB what is the last state that it has, and they will feed the database with all the pings that were missing.

In this way, the collectors backfill any gap created by a restart. If the collector was on a different computer, this will allow the computer of the database to be taken offline for a few hours with no impact on the data. In practice, it is expected that the machine hosting the database would also host a collector, and of course that collector would go down too if the machine itself powers off. 

For the collector, it is a bit more complicated to be able to restart it without having an impact on the data. The most important part is that the database actively balances the number of pings across the available collectors \- so if one shuts down unexpectedly, the remaining should get automatically instructed to ping faster to compensate.

The proper update process looks like this:

1) Replace/Update the collector binary  
2) Start a new collector instance, without terminating the old one.  
3) The collector needs to communicate with the update/restart script to tell when it is ready.  
4) The database should already have tried to balance the two collectors together.  
5) Once ready, the restart script should send SIGHUP to the old collector process.  
6) On SIGHUP, the collector needs to signal the database that it is shutting down, and the database should smoothly remove the load off the process over \~2 seconds.  
7) Once the database reports zero to do to the old collector process, the process closes itself.  
8) The restart script, if it sees that 5 seconds have passed without stopping the old process, it will send SIGINT, then SIGTERM, then SIGKILL, to ensure we close it in less than 30 seconds.

With this mechanism, updating either the database or the collector should lead to zero downtime, with the graphs and data points continuing as if nothing happened.

## UDP as heartbeat

Turns out we can use the same port for TCP/IP and for UDP/IP, and they are separate channels.

UDP is good for probing where the database is, and if it’s actually running. It can also detect errors much quicker than TCP.

Also, UDP shouldn’t be needed unless the LAN has faulty cables/switches, or the machines randomly go out of power. TCP works perfectly well as long as the programs close more or less gracefully.

We will have to see how bad it is to make the collectors reconnect automatically to the DB, or how fast they detect the connection to be down.

Also note that we can make use of broadcast in UDP to announce/ask for a database. Let’s say that the DB is on port 7777 (the actual port is still to be defined). If a collector cannot connect to a database, it could broadcast a UDP message to port 7777 every second, asking for a database. If a database is on the same network, it would reply; then we would know where the database is, what is their IP. This also would be useful for a multi-master database setup in the future, because collectors wouldn’t need to keep a list of database IPs. It also works for DHCP quite well, if the DB changes IP, collectors will find it too.

## Dynamic configuration

We want to keep the deployment as easy as possible, and easy to tune on the fly. Because of this, we want the different apps and services to only contain the following configuration on disk:

* database IP/port to connect  
* username  
* shared secret

Everything else would be dynamic, meaning that the configuration would be stored within the database service.

The database must provide mechanisms for reading the config, writing configs, and subscribing to changes.

Each collector needs to be “self-aware” of the following:

* Their Source IP address \- for the main networking interface  
* Their hostname  
* An installation UUID  
* Their Process ID (pid, or equivalent) \- this is to differentiate between two collectors running from the same host+installation+config.  
  * alternatively, the Src Port used to connect to DB could be used instead

These should be used when communicating with the DB, and to know what configurations or “commands” are for them.

The most important configuration stored in database is:

* List of hosts to ping (IP addresses)  
* Frequency to ping each host (how many packets per second)

These can be “general” (for all collectors), or per collector.

The idea is that the user will set in the GUI the general ping speeds and targets, and the database will automatically balance the load across available collectors.

This means that if we have 2 machines in the LAN with collectors, and we add their IP addresses to the host list to ping, we will have 4 ping combinations:

* Host 1 \-\> Host 2  
* Host 2 \-\> Host 1  
* Host 1 \-\> Host 1  
* Host 2 \-\> Host 2

The machines pinging themselves could be used as an indicator of CPU strain, and as a self-check. If we set 100 pings/sec for each target, then each combination src-dst would get 50 pings/sec, so half from itself, half from the other machine.

Another thing that probably will need extra configuration is how to classify the src-dst pairs, because there are several types of connections:

* Self checks: when Src IP \== Dst IP.  
* LAN checks: when Src IP is on the same /24 mask as Dst IP   
  * (Here the mask might need to be configurable)  
* Router checks: When Dst IP \== Router IP  
  * Router IP will need to be configurable; or set some flag on the dst/target host indicating this is the router.  
* Internet checks: when Dst IP is not on the /24 mask of the Src IP.

When showing a realtime network health it will be important to show up to what level it works. Because we don’t want to show a “25% packet loss” just because the internet is 100% down, and everything else works. That is not true.

Note that with the debian package “net-tools” we have the “route \-n” command that shows the mask and gateway, and this is accessible to regular users, no root access is needed to read this. 

### The Configuration Model

To address the challenge of dynamic, reliable configuration, zzping will adopt a three-tier configuration model. This approach cleanly separates the concerns of bootstrapping, user intent, and real-time operational state.

1. **Connection Config (Bootstrap):**  
   This is a minimal, static file (connection.ron) that resides with each zzping component (database, collector, GUI). It is edited manually by the user and contains only the essential information needed to establish a connection to the database: its address, the username, and the shared secret.  
2. **Intent Config (Master Configuration):**  
   This is the single source of truth for the *desired state* of the entire system, stored and managed by the zzping-database service. It will be a human-readable file (e.g., intent.ron) that includes the list of monitoring targets, target-level aggregate ping rates, collector tags, and "Bad Minute" SLI rules. All changes to this configuration are made dynamically through the GUI/CLI.  
   * **Future-Proofing for Replication:** To support potential future multi-master replication, this configuration will be versioned internally with a last\_modified\_utc timestamp. This enables a simple "last write wins" strategy for achieving eventual consistency between database replicas. This implies a soft requirement for all machines running zzping components to have their clocks reasonably synchronized via a standard protocol like NTP.  
3. **Effective Config (Live Operational State):**  
   This is an ephemeral, in-memory configuration that is continuously recalculated by the database. It represents the real-time instructions sent to each collector, derived by applying the Intent Config to the current state of the system (e.g., which collectors are online and healthy). For example, if the *intent* is to ping a target at 100 pps and two collectors are available, the *effective* config might instruct each collector to ping at 50 pps. This state is lost on a database restart and is regenerated from the Intent Config, making the system resilient to collector connection changes and database restarts.

## Authentication

Despite zzping looks innocuous enough, we will have some basic level authentication. This is because there’s in fact a security issue with this model: If the database TCP socket accidentally is reachable to a bad actor, they could tell zzping to perform a Denial Of Service attack against any target host, that when properly done, it could take down a network device. It also opens the door for an attacker to mess up with the different apps and services that are using the database for communication, and if there were any bugs, they could potentially exploit it.

How does this happen? Either by accidentally exposing the zzping database to the internet (NAT?) or by having the bad actor already inside the LAN. For example a previously compromised computer.

The objective isn’t to perfectly remove all security problems, but just to make the most obvious ones way more difficult to exploit. After all, we consider the LAN a trusted area.

The main idea is to have HMAC challenge-response authentication. This means that both the server (database) and the clients (collector, gui) store the same secrets in plain text on disk.

The approach of challenge-response prevents an attacker from sniffing the TCP/IP data and knowing the secrets to authenticate into the database. The caveat is that the same secret is stored too in the server in plain text. This is different from the typical approach where only a hash (md5/sha1/sha256) is stored in the server \- this is not the case here. In our case the same “password” is stored in plain text on the server too.

A typical basic setup would be to have a single “admin” user, and a shared secret (password) that we create on the first installation, then the user just copies the same config file to all the services and apps deployed.

Initially we will support just 1 user with 1 secret and that’s it. Because this setup already covers 99% of the security risks we’re concerned about.

However, we do plan to have support for multiple users to separate collectors from GUI/CLI apps, and within GUI apps, to separate admins from just readers. For example if we’d want to share read-only access to see the ping data to other people in the same household. This implies ACLs and roles. 

### How does HMAC challenge-response works

When connecting, the client will send a hello message to the DB identifying itself, plus a random string that will be different for each connection. The database will reply, identifying itself, and give its own random string to the client.

\> HELLO collector-client v0.3.0 2025-01-01T12:59:59.00054 “clirandom-113882383842FE”

\< HEY database-server v0.3.0 2025-01-01T12:59:59.02321 “dbrandom-999454932AB”

Now the client uses this information to create a hash with HMAC, joining the data available and delivered by the database, with the shared secret. This will create a custom hash message that shows to the database that it is in possession of the shared secret without actually sending it. This is important because the channel is not encrypted.

\> CLI-AUTH-v1 “admin-user” “0ab56f3863bc43”

The database will compute the same, and should be able to verify that they get to the same “auth-token”, that will authenticate the client.

If it didn’t match, the DB will reply with Error, and close the connection: \< CLI-AUTH-ERROR err-code “error message”

If it did, now the database needs also to prove to the client that it has the same auth secret. So it will create its own version of the HMAC, which should follow a slightly different method to ensure it creates a different token, and reply back

\< CLI-AUTH-OK; DB-AUTH-v1 “admin-user” “15fc4a90943aa3”

Now the client has to perform the same calculation too, and validate that the database, in fact, has the shared secret too. This prevents someone from spoofing the database and just replying “OK” to the client, because they would gain control of the collectors.

If all goes correctly, then:

\> DB-AUTH-OK

And the client and server are fully connected for this TCP/IP session.

Some additional notes here:

* HELLO message / identification might be redundant: Yes, but it’s better to do it for other purposes, such as logging, or negotiating a protocol in the future.  
* The server should provide the only nonce for the client's authentication, and the client should provide the only nonce for the server's authentication. They should not be mixed.  
* The actual hashing formula is kept ambiguous at this stage: It is an implementation detail. It could be something like “client” \+ $db\_nonce \+ $secret / “server” \+ $client\_nonce \+ $secret.  
* Auth steps could be combined into less messages: True, but we prefer to keep things simple even if it takes more round-trips. It is an unnecessary optimization.

## GUI for showing million data points

Most GUI frameworks are not prepared to show so many points so fast. zzping uses iced because it allows for very fast drawing. Other frameworks did not show good performance here.

However this was true years ago at iced 0.4, a recent attempt to upgrade to iced 0.13 showed that the library isn’t mature, a new canvas bug that makes it unusable until fixed, and it requires good gpu drivers which is a non-sense.

We can stick to iced, or examine other GUI frameworks, taking into account the problem of drawing fast.

Another common problem is the systray icon, which in Linux has had some trouble in Gnome 3\. 

Some candidate alternatives:

* egui \- [https://docs.rs/egui/latest/egui/](https://docs.rs/egui/latest/egui/) \- [https://www.egui.rs/](https://www.egui.rs/)   
  * systray icon via tray-icon crate  
* Slint \- [https://docs.slint.dev/latest/docs/slint/](https://docs.slint.dev/latest/docs/slint/)  
  * no tray icon support on docs. Seems to use Qt as a backend (or other backends and people complain they’re blurry)  
* gtk-rs \- [https://gtk-rs.org/](https://gtk-rs.org/)   
  * bindings for GTK 4\. While I dislike they’re bindings... it might be what we need.  
  * still problematic because it dynamically links to LGPL libraries that need to be bundled for windows  
  * cross-compiling for windows is going to be hard.  
  * Unclear the speed for rendering in canvas  
* tauri \- [https://tauri.app/](https://tauri.app/)   
  * actually is an app-in browser, but very popular, lightweight  
  * html+css+js  
  * performance for canvas [can be very bad](https://www.reddit.com/r/tauri/comments/1jur0hl/canvas_rendering_performance_in_tauri/) in some situations (nvidia+x11)  
  * does support systray out of the box  
* Rerun \- [https://github.com/rerun-io/rerun/](https://github.com/rerun-io/rerun/) \- [https://rerun.io/viewer](https://rerun.io/viewer)  
  * not a GUI library, but a framework for data visualization \- very fast  
  * no systray support \- an extra app is needed to do this  
  * default operation is to have .rrd files on disk, for historical views  
  * it actually seems to be working mainly on RAM, and we feed what we want, but it is not clear.  
  * It’s also the work of Emil, the same author of egui\! \- it’s also made in egui

From all the options I have seen, egui is the only sane one it seems. Everything else is full of caveats.

And ReRun might be worth taking a look, because data visualization is exactly what I want, and it comes with lots of stuff already baked in \- interactivity, zoom, panning...

ReRun has lots of potential problems, such as that the connection is reversed, is DB to GUI instead; that it has a particular format RRD, that it is a standalone app that you customize... 

However \- worst case scenario, it proves that egui works for what we want to do in zzping. We tried the ReRun as a native binary and it works flawlessly.

We can also use ReRun as a crate library to save to RRD files that we can later open in the ReRun app. That could be interesting to see how compact the format is, and how usable it is.

**First Proof of Concept test**

Our first demo app zzping-viewer demonstrates that it can draw over 800k points seamlessly, even without any kind of caching aggregated values. Draws really fast and it moves fast.

The PoC app however has a lot of annoying bugs specially on usability, which are normal for a first dumb attempt.

The programming language, the API, seems easy enough and reasonable.

## Simple Installation and Deployment

Having a complex system with multiple components is hard to make it easy for a user to get it running correctly and securely. A user should be able to go from a downloaded release to a fully functional, default-monitoring setup with minimal friction. 

**The Challenge: The "First Light" User Journey**

The critical user journey we must solve is that of a first-time user. They have downloaded zzping and want to see it monitoring their network. A successful journey means they can achieve this without needing to manually edit configuration files, discover IP addresses, or understand the system's architecture. The system must be useful and secure "out of the box."

**The Proposed Solution: A Modular, Script-Based Installation**

To address this challenge, we will adopt a multi-stage installation process that separates configuration discovery from privileged system setup, ensuring both security and transparency.

**1\. Configuration Generation (zzping-init)**  
The process begins with a user-mode, unprivileged helper binary.

* The user runs ./zzping-init, which performs discovery tasks without requiring root access.  
* It programmatically discovers the host's primary IP address and default gateway (router).  
* It generates a cryptographically secure random shared secret.  
* It writes these discovered settings and the secret into a single, human-readable configuration file: install\_config.ron.  
* A sample install\_config.ron will also be provided, allowing advanced users to bypass this step and create their own configuration for automated deployments.  
* We will provide a default set of hosts to ping initially (1.1.1.1, 8.8.8.8, 9.9.9.9)

**2\. Component Installation (Modular Scripts)**  
The actual installation is handled by a set of simple, auditable shell scripts that consume the install\_config.ron file. This modularity allows users to install only the components they need on a given machine.

* install-database.sh install\_config.ron: Installs the database service.  
* install-collector.sh install\_config.ron: Installs the collector service.  
* install-clients.sh install\_config.ron: Installs the GUI and CLI applications for the current user.

These scripts will be responsible for creating users, copying binaries, setting up service files, and writing the final configuration needed by each component, including the pre-configured connection file for the GUI.

**3\. System vs. User Installation Modes**  
The service installers (database and collector) will support two distinct modes:

* System Mode (Default): Run with sudo, this mode installs the components as system-wide systemd services. This is the recommended approach for always-on services.  
* **User Mode (--user flag):** Run without sudo, this mode installs the components as systemd user services that run under the current user's account. This provides a fully sudo-less installation path, though it may require the user to enable "lingering" (loginctl enable-linger) to ensure services persist after logout.

**Limitations and Scope for v0.3**

* **Operating System:** The initial implementation of this installation process will be targeted exclusively for modern **Linux distributions using systemd**. While the core zzping applications are being written with cross-platform compatibility in mind, creating robust installers for macOS and Windows is a distinct and significant undertaking that is out of scope for v0.3.  
* **Network Discovery:** The collector's ability to automatically detect the default gateway relies on platform-specific APIs. While this is well-supported on Linux, the system must handle discovery failures gracefully by logging a warning and proceeding without that specific target.

**Pending challenge:**

* How to easily update components. Say we have a new version, or just we coded something on the git repo. How do we quickly get the changes to all machines? We don’t want to create a security risk.

## Defining "Bad" Performance

Graphs are good for analyzing in depth, but it's hard to get a quick, at-a-glance summary of network quality. To make the history useful, we need to answer a simple question like, "Was my connection good yesterday?"

The idea is to formally define what a "Bad Minute" of network performance looks like and have the system automatically flag those periods. This would let the GUI provide powerful summaries, like "You had 17 bad minutes yesterday between 2pm and 3pm," or even calculate a reliability score for the month, like "Your internet connection met its quality target 99.85% of the time."

Challenges in Defining "Bad":

* "Bad" is subjective and contextual. Depends on what we want to do, and our expectations of the network. However, we want to keep things simple so we define a single rule of 500ms RTT for what is bad, such that it is quite clear-cut that if it happens, it is always bad regardless of conditions. We should leave this parameter configurable, so users can tune it according to their expectations. (note that 500ms RTT might be expected for some satellite connections and congested networks)  
* Averages are misleading. Just looking at the average ping time can hide serious problems. A single, massive 2-second lag spike might get smoothed out by thousands of good pings, but that one spike is what kicked you out of your game. We need metrics that are sensitive to these painful outliers.

**A Potential Approach: SLIs and "Bad Minutes"**

We can borrow some concepts from the world of Site Reliability Engineering (SRE) and adapt them for home use. The plan would be to define a set of clear indicators (Service Level Indicators, or SLIs) and then set thresholds for them.

1. **Define the Indicators (SLIs):** We'd focus on the two things a user actually feels:  
   * Latency: Instead of the average, we should track a high-percentile latency, like the 95th percentile (p95). This means we're measuring the experience of the worst 5% of packets. It's a much better indicator of perceived lag than the average.  
   * Packet Loss: This is more straightforward—a simple percentage of packets lost over a given time window.  
2. **Define the Thresholds:** The user (via the GUI) should be able to set simple rules. A "bad minute" is a one-minute window where any of these rules are broken. For example:  
   * A minute is "bad" if p95 latency \> 500ms OR packet loss \> 3%.  
   * NOTE: This does not cover “slightly bad” but not bad enough for a bad minute. We might need to consider how to transfer this into “bad hour” such that we could incorporate tighter metrics at hour resolution.

Implementation Challenges and Ideas:

* Where does this logic live? This seems like a perfect job for the zzping-database service. As it processes and aggregates the incoming raw data into one-minute chunks, it could simultaneously check this data against the user-defined "bad minute" rules.  
* How do we store the results? Calculating this on the fly for a full-year query would be too slow. Instead, when the database detects a bad minute, it could write a small, separate "event" record to a dedicated log or table. This log would be very compact and extremely fast to query. We could then easily count these events to generate daily, weekly, or monthly reliability reports.

Open Questions and Further Considerations:

* Default Rules: What are sensible default rules for a typical user? We'll need to research and provide a good starting point.  
* Granularity: Is a "minute" the right time window?

It is important to clarify the primary goal of this feature. The 'Bad Minute' SLI is **not** designed to measure subjective user experience or whether the network is 'up to one's taste.' Instead, its purpose is to create an objective, durable signal of **technical failure**—a clear-cut indicator that the network is broken in a way that survives long-term data aggregation and can be reliably counted over months or years.

The architecture—separating raw data storage from SLI calculation—is designed to be flexible. If future needs require more nuanced definitions (e.g., user-defined profiles for 'Gaming' vs. 'Streaming'), the system can re-process the raw historical data to apply new rules without ever losing the original ground truth. For v0.3, however, the focus will remain on a single, globally configurable set of thresholds for detecting clear-cut network degradation.

## System Health and Observability

A monitoring system that cannot report its own status is not trustworthy. The most significant challenge in any monitoring tool is the "ambiguous blank graph": when data stops appearing, is it because the monitored network is down, or because the monitoring tool itself has crashed? Without a clear answer, the user cannot trust the data they see—or don't see.

The v0.3 architecture addresses this challenge by treating the health of the zzping system itself as a first-class metric, creating a parallel "health data" pipeline alongside the primary "ping data" pipeline. The core principle is to provide clear, at-a-glance feedback on the system's own operational health, allowing users to instantly differentiate between an external network failure and an internal system failure.

This will be achieved through a clear separation of responsibilities:

* **Collector's Role: Self-Reporting**  
  In addition to streaming ping data, each collector will periodically report its own operational health metadata to the database. This is more than a simple heartbeat; it is a rich status update that will include its connection state, current in-memory buffer usage (to detect if it's falling behind), and any critical errors encountered (e.g., a fatal, unrecoverable failure to create ICMP sockets).  
* **Database's Role: Central Health Aggregator**  
  The database will be the central hub for system health. It will receive and track the status of all connected collectors, maintaining a real-time view of each component's state. It will apply its own logic to determine if a collector's status is Online (reporting normally), Stale (has not reported within the expected interval), or Error (has reported a critical failure). This consolidated system-wide health information will be exposed via a dedicated API.  
* **GUI/CLI's Role: The User-Facing Display**  
  The user-facing applications are the final, crucial link. They will consume the health API from the database to provide the user with a clear view of the entire zzping system's status. This could manifest as a simple status icon (green/yellow/red) in the GUI's system tray or a dedicated status panel.

The outcome of this approach is a system that is fundamentally more robust and trustworthy. A flat graph showing 100% packet loss paired with a **green** system health indicator gives the user high confidence that the network is truly down. The same flat graph paired with a **red** indicator and a message like "Collector 'Gaming-PC' is offline" provides an immediate, actionable insight for troubleshooting the zzping deployment itself.

## Data Usability and Context

A successful monitoring system does more than just collect and display data; it helps the user turn that data into actionable insights and communicable evidence. While the v0.3 architecture focuses on the core tasks of robust data collection and real-time graphing, a key long-term challenge is to ensure the vast amount of collected data remains useful, understandable, and interoperable over time.

This challenge can be broken down into several key requirements that the zzping platform should aim to address in the future. These are presented here as open-ended problems to guide the system's evolution beyond the initial v0.3 release.

**1\. The Requirement for At-a-Glance Insights**

* **The Problem:** While detailed graphs are essential for deep analysis, they are inefficient for answering high-level questions like, "How has my network's reliability been this month?" A user should not have to manually inspect weeks of data to get a simple summary.  
* **The Unsolved Need:** The system needs a way to distill long-term data into high-level, easily digestible dashboards. This implies a future need for a user interface that can present reliability scores (e.g., "% of good minutes") and trend charts (e.g., "bad minutes per day") powered by the database's pre-calculated SLI events.

**2\. The Requirement to Preserve Human Context**

* **The Problem:** A graph can show a massive latency spike, but it can't explain *why* it happened. Was it an ISP maintenance window? A large file download? A router reboot? Without this human context, the data loses much of its diagnostic value over time.  
* **The Unsolved Need:** The system should provide a mechanism for users to **annotate** the data timeline. A user should be able to mark a specific event with a simple note. This would require a way for the database to store and retrieve this user-generated metadata alongside the machine-generated ping data, turning a simple data log into an invaluable historical record.

**3\. The Requirement for Effective Communication**

* **The Problem:** A detailed zzping graph is a powerful tool for a technical user, but it can be overwhelming and unconvincing when shared with a non-technical person, such as an ISP support agent. A user needs a way to translate the system's findings into a simple, universally understood format.  
* **The Unsolved Need:** The system should support **specialized export formats** designed for human communication. Beyond a simple CSV export for analysis, a key idea is a "simulated CLI" export, which would generate a text output that mimics the familiar format of a standard ping command. This would provide users with a powerful tool for sharing clear, concise evidence of a network issue.

# Appendix: Core Use Cases

(Section written by AI, needs review \- not sure how relevant is this)

To illustrate how the architectural components work together to solve real-world problems, this section outlines two key scenarios a user might encounter. These use cases demonstrate the value of the system's design in providing clear, actionable insights into network behavior.

---

## Use Case 1: Diagnosing an Internet "Micro-Cut"

**The Problem:**  
A remote worker is in an important video conference when their connection freezes for a few seconds. By the time they can run a manual ping command, the problem is gone, and everything looks normal. They suspect a brief, total internet outage, but have no way to prove it or differentiate it from a simple Wi-Fi glitch.

**The zzping Solution:**

1. **Continuous Monitoring:** zzping-collector has been running continuously in the background, pinging the local router and three different internet targets at 20 times per second.  
2. **Immediate Insight:** The user opens the zzping-gui. The "Bad Minute" summary immediately flags the exact minute the video conference froze.  
3. **Root Cause Analysis:** The user clicks on the flagged minute, and the graph instantly zooms to that time window. The visualization clearly shows:  
   * **Stable Local Network:** Pings to the local router (192.168.1.1) remained stable and fast (\<2ms).  
   * **Correlated Internet Failure:** Pings to *all three* diverse internet targets (e.g., 1.1.1.1, 8.8.8.8, 9.9.9.9) simultaneously show a complete lack of replies for a period of approximately 800ms.  
4. **Actionable Conclusion:** The data proves the issue was not the user's Wi-Fi or local network. It was a "micro-cut"—a short-lived but complete loss of the internet connection. The user now has a timestamped event and graphical evidence to provide to their ISP, transforming a vague complaint into a specific, data-backed report.

---

## Use Case 2: Differentiating a PC Problem from a Network Problem

**The Problem:**  
A user experiences intermittent, severe lag spikes while online gaming. They aren't sure if the problem is the network, their internet connection, or if their own high-end PC is stuttering under heavy load (e.g., due to background processes or driver issues).

**The zzping Solution:**

1. **Multi-Collector Setup:** The user is already running a zzping-collector on their gaming PC. To isolate the problem, they run the simple install.sh script on a low-power, always-on device on their network (like a Raspberry Pi), configuring it to connect to the same zzping-database.  
2. **Automatic Load Balancing:** The database detects the new collector. The user has a rule to ping the game server at 50 pps total. The database automatically splits the load, instructing the PC collector to ping at 25 pps and the Raspberry Pi collector to ping at 25 pps.  
3. **Comparative Analysis:** In the zzping-gui, the user can now view two graphs side-by-side for the same destination:  
   * **Graph 1:** Pings sourced from the Gaming PC.  
   * **Graph 2:** Pings sourced from the Raspberry Pi.  
4. **Actionable Conclusion:** During the next lag spike, the user observes that the graph from the Raspberry Pi remains perfectly stable, while the graph from their gaming PC shows a massive RTT spike. This provides conclusive evidence that the network and internet connection are fine. The problem is localized to the gaming PC itself, allowing the user to focus their troubleshooting efforts on system performance (CPU, drivers, background tasks) instead of incorrectly blaming their ISP.

# Appendix: Frequently Asked Questions (FAQ)

(to be revised \- this section is written by AI)

This section addresses common questions and critiques regarding the architectural choices for v0.3. Its purpose is to document the rationale behind key decisions and explain why certain alternatives, while valid, were not chosen for this iteration.

---

#### **Q1: The vision involves building a custom database. Isn't that a massive undertaking, fraught with risks like concurrency issues, complex query logic, and crash recovery?**

**The Critique:**  
Building a database from scratch carries significant risk. A custom engine requires solving complex, well-understood problems like concurrency, query logic, and crash-safe persistence, which could derail the entire project.

**The Rationale:**  
This is a valid and critical concern. The decision to build a custom engine is only viable because its scope is **deliberately and severely limited**. The zzping-database is not a general-purpose time-series database; it is a purpose-built engine hyper-optimized for a single task. We mitigate the primary risks with the following non-negotiable design principles:

1. **No Complex Concurrency:** The database will operate on a **single-writer, serial-query model**. A single thread will handle all data ingestion from collectors, and all user queries will be serialized. This completely avoids the need for complex locking, transactions, or race condition management.  
2. **No Complex Query Engine:** The query interface will be minimal, supporting **only time-range and resolution-based requests** (e.g., get data from time A to B at resolution C). There will be no ad-hoc query engine. New statistical views or insights must be generated by pre-calculation during offline aggregation.  
3. **Simple, Robust Crash Recovery:** We will use an **append-only file writing strategy**. In the event of a crash, the database will perform a recovery check on startup by scanning its data files to find the last valid record and simply truncating any trailing corrupt data. This avoids the complexity of a Write-Ahead Log (WAL) while providing sufficient robustness for the target use case.

**Actionable Notes:**

* These principles should be explicitly stated as core tenets of the database design in the main body of the document to manage scope and expectations.

---

#### **Q2: The 'one file per pair' storage model seems inefficient for certain queries. Wouldn't a different model be better?**

**The Critique (Q2-A):**  
A file-per-pair structure requires opening and seeking in multiple files for any query that looks at the whole network state at a specific time. A time-chunked model (one file per hour for all pairs) would be much more performant for these common queries.

**The Rationale:**  
A time-chunked storage model is a valid architecture that can offer superior performance for multi-pair, time-aligned queries. However, for v0.3, we have chosen the **file-per-pair model for its implementation simplicity**. Given the expected scale of a home network (typically \<10 pairs), the performance overhead of opening multiple files is expected to be negligible on modern hardware. This decision prioritizes a simpler, more robust initial implementation, while acknowledging that a time-chunked model could be a potential future optimization if query performance becomes a bottleneck.

**The Critique (Q2-B):**  
Wouldn’t storing data this way create thousands of files over time, causing filesystem performance issues?

**The Rationale:**  
Yes, this is a known limitation of many filesystems. To mitigate this, the file-per-pair data will be stored in a **hierarchical directory structure**, likely organized by year/month/day. This ensures that the number of files in any single directory remains well within performant limits.

**Actionable Notes:**

* The document should briefly mention the time-chunked model as a considered alternative in the "Storing data to disk" section, along with the rationale for deferring it. The directory structure should also be mentioned.

---

#### **Q3: Why create a custom TCP protocol? Aren't you just reinventing the wheel when mature frameworks exist?**

**The Critique:**  
Building a custom network protocol from scratch means solving problems like message framing, versioning, and request multiplexing, all of which are handled by mature RPC frameworks.

**The Rationale:**  
While existing frameworks offer significant benefits, a **custom TCP protocol was chosen to maintain complete control and minimize dependencies**. The rationale is twofold:

1. **A Precisely Defined Security Model:** Our security is based on a **"trusted LAN" threat model**, which makes several explicit assumptions:  
   * **Scope:** Communication is assumed to occur on a private network. Scenarios involving traffic over the public internet are out of scope and should be secured by the user with a VPN or SSH tunnel.  
   * **Threat:** The primary goal is to prevent unauthorized **control** of the system (e.g., a malicious actor configuring a DoS attack), not to protect the data-in-flight from a sophisticated attacker already on the LAN.  
   * **Mechanism:** Our two-way, shared-secret HMAC authentication is perfectly tailored to this model. Many RPC frameworks either default to simpler security (plain-text tokens) or expect a more complex setup (TLS certificates) and often do not natively support the mandatory server-side authentication that our protocol requires.  
2. **Control and Simplicity:** This approach avoids pulling in a large ecosystem and its associated boilerplate. While we are responsible for message framing (e.g., via length-prefixing), this trade-off is acceptable for the total control and leanness it provides, especially when paired with powerful Rust libraries like serde and tokio.

**Actionable Notes:**

* A new top-level section titled "Communication Protocol" should be created in the main document to formalize these decisions.

---

#### **Q4: How does a collector reliably figure out its own IP address and find the database initially? This seems like a fragile part of the setup.**

**The Critique:**  
Auto-discovery of a collector's "correct" IP is a notoriously difficult problem on multi-homed machines, and relying on a static IP for the database can be brittle.

**The Rationale:**  
This is handled with a simple, robust, two-tiered approach that covers the vast majority of use cases while providing a fallback for complex ones.

1. **IP Discovery:** The primary method is for the collector to **determine its source IP from the TCP socket it uses to connect to the database**. This is an elegant solution that is correct for most simple networks. For multi-homed machines, a simple, optional source\_ip\_override in the collector's local config file will serve as a fallback.  
2. **Database Discovery:** The primary bootstrap mechanism is a **static IP address for the database**, configured in the collector's local file. While requiring a static IP is a reasonable and reliable requirement for v0.3, a future version may explore an optional **UDP broadcast-based discovery** for a zero-config experience. The implementation could involve the database listening for broadcast probes from new collectors and responding directly. This approach simplifies initial setup, though its implications for more complex scenarios like multi-master replication would require further investigation.

**Actionable Notes:**

* These discovery mechanisms should be detailed in the "Collector Service" section of the main document.

---

#### **Q5: A simple disk space limit feels unpredictable. Wouldn't a time-based policy ('keep raw data for 7 days') be more intuitive for the user?**

**The Critique:**  
Time-based retention policies are easier for users to understand than abstract space limits, which can be hard to estimate.

**The Rationale:**  
While time-based policies are intuitive, they don't address the user's most fundamental constraint: **finite disk space**. A user is more likely to know "I can spare 100 GiB" than to accurately estimate the space consumed by "90 days of data at 120 pps". Therefore, we are implementing a **hybrid model that combines the best of both approaches**:

1. The user sets a simple, predictable **disk space limit**.  
2. When this limit is approached, the database automatically prunes the oldest data using a **tiered, time-based strategy**: raw pings are deleted first, followed by minute-level aggregates, and finally hour-level aggregates.

This model respects the user's primary constraint while intelligently preserving long-term historical trends, providing the maximum possible value within the allocated storage.

**Actionable Notes:**

* This hybrid retention policy is a key feature and should be a prominent part of the "Offline aggregation" or "Storing data to disk" section.

#### **Q6: For maximum data resolution, shouldn't the system perfectly stagger pings from all collectors?**

**The Critique:**  
A system with multiple collectors could achieve the highest possible time resolution if it ensured all pings were perfectly spaced out in time. A lack of coordination could lead to collectors firing pings simultaneously, reducing the effective sample rate.

**The Rationale:**  
While perfect, centrally-coordinated staggering offers theoretical benefits, the implementation complexity is significant. It would require the database to act as a real-time pacemaker, adding latency and a new class of failure modes to the system.

For v0.3, we will adopt a **"best-effort" staggering** approach, if needed. The goal is to avoid complexity while still achieving good results. We anticipate that natural clock drift between collectors, combined with randomized startup delays, will provide sufficient de-synchronization. This can be enhanced with a minimal-complexity feature where the database assigns a one-time, random phase offset to each collector upon connection, further ensuring pings are not clustered. This provides most of the benefit without the cost of a complex synchronization protocol.

---

#### **Q7: The collector/database architecture seems perfect for monitoring a remote location, like a parent's house. Does zzping include features for secure access over the internet?**

**The Critique:**  
The distributed model is ideal for remote monitoring. Users will naturally want to connect collectors or GUIs to a database over the public internet, which requires robust security like TLS.

**The Rationale:**  
No, providing built-in, secure remote access is explicitly a **non-goal** for v0.3. The project's security model is designed for a **trusted local area network**, and the custom HMAC protocol is tailored for that specific environment.

Users who require remote access to their zzping-database are expected to use standard, secure, and well-established industry tools such as a **VPN or an SSH tunnel**. This approach has several key advantages:

1. **Avoids Reinventing Security:** Building and maintaining a secure public-facing service is a massive undertaking. Leveraging proven technologies like SSH and VPNs is far more secure than any custom solution we could build.  
2. **Maintains Project Focus:** It keeps the project's scope focused on its core mission of high-performance network monitoring, preventing "scope creep" into the complex domain of secure network transport.  
3. **Empowers Users:** It trusts users to employ the correct, standard tools for the job, which is a more robust and flexible approach in the long run.

---

#### **Q8: What about advanced analysis like DCT/FFT for compression or visualization?**

**The Critique:**  
The v0.2.2 codebase experimented with signal processing techniques like DCT for compression, but they proved complex and were not clearly successful. Given the project's new focus on simplicity, are these techniques still relevant for v0.3?

**The Rationale:**  
This is a key insight. As a compression method for raw pings, these advanced techniques are **out of scope** for v0.3. They introduce significant complexity and potential for data loss, which contradicts the new architecture's principles. The simple 4-byte format is the priority. If used at all, advanced compression would be a private, internal implementation detail for the long-term archival of old, aggregated data within the database.

However, these techniques remain relevant when addressing a different, unresolved challenge: **efficiently visualizing heavily zoomed-out datasets.** When a user wants to view a year of data on a single screen, the system cannot simply plot billions of raw points. Standard methods like downsampling or averaging can hide the very "micro-cut" events zzping is designed to find. Signal processing offers a way to create a smaller, visually faithful summary of the data that preserves these critical events.

The architectural decision of **where this analysis logic should live**—whether it is a complex client-side task for the GUI or a specialized "visual summary" API in the database—remains an **open design question** for future implementation phases. The v0.3 architecture, by separating the database and GUI, provides the flexibility to implement this logic in either component as performance and complexity trade-offs become clearer.

# Appendix: Learnt lessons from proof of concepts

date: Aug 29, 2025

**Sending ICMP**

**Tokio** is giving awesome results, with near zero CPU load. Tokio’s sleep precision is way too bad, but combined with a bit of thread::sleep we get \<100us deviation, which is excellent.

Why is it excellent: for storage compression. If we can decide exactly when to send the packets, we can make it in such a way that we do not need to store when they were sent, only their RTT.

**Unprivileged ICMP:** Working like a charm, no issues. Although one thing is weird, that some packets seem to return 50% faster than most of them, in some cases (suspiciously too fast). Not sure if this is an issue with how we are pinging, or if this is natural/normal network behavior.

Unprivileged ICMP is very easy to set up user-side and avoids the use of sudo commands to add permissions to the binary itself, which to me, looks safer and simpler. The user still needs to configure via sudo the system to allow for this, but seems pretty normal to me.

**Other ping backends might be needed later.** We have not explored how to do an interchangeable backend. In some cases, raw sockets might be the easier solution. Also there’s the option for UDP pings, but this requires setting up the server on the other end too.

For now I want to focus on user-level privilege installs, and simple stuff that works too on my machine while developing. And, unprivileged ICMP hits the spot.

**ReRun as a GUI app for zzping**

This is mostly discarded. While ReRun is awesome, the file formats are not well suited, the amount of data we manage doesn’t seem to work smoothly in the UI and would need manual workarounds by a user. 

Of course, the export to ReRun files is interesting, or even maybe exporting to it via HTTP. Or other file formats. But all of this becomes very low priority because I see very little usage.

But exporting individual pings looks way too much information, and if we do this we should probably focus on exporting the per-minute percentiles.

**egui as a GUI library for zzping-gui**

This is a clear yes. It is very fast, and powerful, as demonstrated by ReRun. The initial proof of concept did show that the egui API is mostly easy to follow.

However, the proof of concept we’ve done so far is very unpolished, ad-hoc, and kind of broken from a UX perspective. More work is needed here to understand what are the full requirements for zzping-gui.

There are for sure a lot of problems regarding GUI that we do not know yet because we haven’t reached that point.

**Efficient storage on disk \- the zzp1 format**

The hypothesis that 1-byte per RTT was possible was more than correct, we are achieving \~6.5 bits per RTT. Not only that, the format is chunked at 1-minute intervals and allows for append-only operation, meaning that this can be used directly to the database to store on the get-go. We do not need a 2nd compression pass. The 2nd pass just tweaks the header to leave the data for indexing and aggregated per minute stats, so that we also get cheap lookups.

This format (chunked-v1, \*.zzp1) encodes and decodes roughly at 40 Million RTT/s which ensures files can be parsed in less than half a second.

NOTE: Files are optimized for 24 hour, single src, single dst. They cannot hold more data than 24 hours. They can hold higher ping frequencies, but not longer time intervals. This is designed such that we make 1 file per day.

NOTE: We need to ensure that \*.zzp1 files are self-describing, in that they contain the fields for src/dst and datetime they correspond to. Currently this is in the filename, but we shouldn’t count on this.

The format is inherently lossy, we quantize to get exactly the precision we need. Not more.

Also the format being designed for most of the operations being left-to-right means that it should still be pretty fast on a spinning HDD. This hopefully would allow users to store a decade of raw ping history into a hard drive.

NOTE: It is still unclear if we’d want the quantization parameters to be tweakable. “ln(60000/100+1)/ln(1.001)” returns 6402 as the highest possible symbol (60 seconds RTT). This quantization has typically 2 tweakable parameters: 1\) the lowest measurable RTT (0.1ms now) ; 2\) The max deviation possible for high values (0.1% now). For example, dropping the minimum RTT to 1us might not change the compression rate at all. Changing the max deviation would be able to trade off precision for more compression. But we also need to note that at very high compression rates, the space used by the headers would become significant, meaning they would need to be re-chunked at an hourly level or similar for that to be efficient. 

Is a precision/compression tradeoff something we’d want to be customizable? recompression for archival? Overall the format isn’t suited for this, because it assumes a stochastic process. If we quantize enough, the random side of the distribution gets eventually removed (or it’s irrelevant) and we should look into different techniques for storing the data with higher compactness (or people could remove the raw data and leave the monthly 1-minute aggregates). 

Higher precision than 0.1% / 0.1ms seems also rare to want, since what we’re capturing at that point is mostly white noise, and the quantization shouldn’t be apparent on graphs.

**NOTE: we need better graphs and GUI to show the contents on the data, to gauge if the quantization is visible. If it isn’t visible, then it does not matter. We need a proof of concept to derisk this.**

Anyway, we likely only need 1 setting for quantization; but nonetheless probably the quantizing process should be generalized so it becomes clear how to tweak it, which are the variables and how to control; being careful not to create more than 65k symbols, and that 60s interval do fit.

The format contains a constant send rate mode and a variable rate mode. Constant rate makes use of the fact that the pinger will do its best to send the ping in a “grid”, and it only needs to set the width of the pulse; or say how many there are in the minute. Which avoids saving send times. 

The variable send rate mode is problematic, and easily bumps to 30 bit per RTT. More work is needed to make it good. However we could instead use a completely different strategy, maybe:

* Make/force the pinger to always conform to a grid  
* Simply record a ping as “not sent” instead of drifting everything  
  * Think of a duty cycle. PWM.  
* Or maybe a 1ms grid and encode a lot of “not sent”? That might be worse.

The whole point here is that variable rate is bad and should be avoided as much as possible.

This format contains in the header, once repacked, indexes to all chunks so a reader can easily read from minute X to minute Y. It also contains percentiles \[p00,p10,p20,..,p100\] for each minute. So it already contains aggregation of data. The repacking is expected to happen once the file is “closed for writes”, so in the next day.

The file is also expected to lag 2-3 minutes behind what’s received. We need 1 minute to confirm all packets are in (or claim them lost), plus another minute to create the full chunk with stats. Saving in 1 minute chunks also avoids doing too many fsync operations.

NOTE: packet loss stats are not currently stored in the header. This could be problematic, requiring a full day read just to get an overview of packet loss.

**The challenges of Storage to disk and Indexing are mostly clear now.** 

**Offline aggregation still needs work.**

It’s clear that we still need some kind of monthly aggregation at some point. Although this can come up later, because this seems easy enough to recompute. Just compacting all header information of a month into a single file for easy reading could even be sufficient. It’s just 43830 minutes per month. We could even try to have all src-dst pairs in there. We can most likely delay designing this format to the very end. 

How to aggregate packet loss is still a hard question pending to be solved.

The problem is the sub-minute resolution, how the GUI presents the data, how it does compute it, and how it caches it. Such that, we don’t iterate over big data sets over and over, and zoom transitions are smooth and data visualization is meaningful.

We don’t want to store such high precision stats to disk \- they would easily use more space than the raw data itself. We probably want to store this on memory, on demand. This is still something to think about.

**Mock up / Concept for the GUI we want**

Just knowing how a conceptual GUI would look like, how the user would interact, how it would get the data out of it, would give us invaluable information about all pieces of the system. This is something that has to be done soon \- a mock up, a concept GUI.

Dashboard, real time data, examining old data, how do we zoom and find stuff. How do the SLI indicators work? How does the GUI ask for host:port user:pass? How do we configure hosts and collectors?

**Leaning towards a simpler configuration**

The concept of having a ping rate per host, of load balancing the collectors on different machines... I’m leaning towards dropping it. A typical user mainly needs around 30 RTT/s overall, that’s it. And if there are different machines with collectors, each one probably would be also pinging the same host at 30 RTT/s.

The only caveat is 2 collectors on the same machine \- feeding the same data source. This needs to be taken care of.

But maybe there are no advantages to the other complexity we initially laid out in this document, and it overcomplicates GUI configuration.

**SLI and bad minutes**

We can use the percentiles we already store \[p00, p10, p20, ..\] as a design limitation of what SLI can be chosen, so we can define a rule of p90 \> 500ms OR  packet\_loss \> 3% \- and these would be trivial to compute on the fly just using what we already store. Given there are only 43830 minutes per month, this becomes feasible to compute on the fly.

Later on we probably would want to store them for faster speeds, but this seems very low priority and might even not be necessary.

**Querying aggregates / raw / batch has to be possible**

At the very least the GUI has to be able to query from timestamp to timestamp at a desired resolution level. Not any arbitrary resolution, but one of the specified points. Initially, either raw pings or 1 min aggregates.

A zzping-cli app should be able to query the DB to extract data from point A to point B at either raw or 1 min aggregate level and export to CSV, ReRun or other formats.

**Event annotations**

This requires the full MVP to be in place: collector, database and GUI. Because placing an event only makes sense on a GUI.

An Event would have:

* User text annotation  
* Timestamp point or range \- period of time is referring to  
* Flows this applies to: a host, a src-dst pair, or a tag (ie. the internet)

It’s unclear how we would store this, or query it. It definitely needs additional disk storage. One proposal is to use SQLite for this, but bringing SQL just for this feels... overkill or out of place. This needs a lot of consideration.

But because this cannot be tested until the very end, it doesn’t make sense to spend time thinking on it in the current stage.

date Aug 31, 2025

Spent a few days with Jules AI agent to bootstrap a minimal vertical slice, I’m not sure if I should call this an MVP.

This coding exercise has been done with extensive unit testing, no field tests.

We have now a zzping-collector, zzping-database and zzping-gui. Additionally zzping-lib exists to hold all common code, network protocol and disk formats.

But these are still a far cry from what was envisioned on this document. It is reaching a point where I’m not sure where we are, where are we headed, what are the next steps.

The chunked-v1 storage format performance has dropped to 9 bits per ping (from 6.5), possibly because we changed to a finer quantization. Still not a big deal, and we will roll with it for now.

We added the capability of storing chunked-v1 on a chunk by chunk basis (1 min chunks), it seems to work on unit tests. And in theory it should also copy the aggregate stats and index at the end of the day \- not sure if this is properly tested.

Initially the AI created the network comms using serde+json, but later I made it change to serde+bincode. I still wonder if bincode would be a good option for chunked-v1 to avoid manual serialization, because it feels very error prone. For every protocol, we need a way to guarantee that the reader part is exactly the counterpart of the writer part. And we’re failing; as we touch how these work, the protocols get flaky, with edge cases or hardcoded stuff. This is a problem because it’s very hard to test every case.

I decided to change, on the src-dst pairs, that the SRC would no longer be an IP Address. What’s the IP address of the collector? Well, does it really matter? what it matters is that we do identify the machines properly \- so I thought that using hostnames would be better, so it should remain more consistent. And probably there’s not much point in even using a crate to get the hostname of the PC, maybe it’s just better to manually label the collector.

The collector now accepts multiple targets for ping, and a source\_hostname to identify itself. The ping rate is the same for all targets, which is a simplification that I’m seriously leaning into.

But still the collector doesn’t have a config file, and does not receive instructions from the database. It just boots up with flags, and that’s it.

An important thing here is the timing precision of the pings \- we’re back to square one. Not only the original code for fine timing wasn’t ported (although we tried, and we have something there now...), it is also not clear if it’s going to work for multiple hosts to ping at the same time. std::thread::sleep will probably trip Tokio, and we do not have guarantees that all hosts would be pinged when they should. This is something we need to go back and verify.

There is some kind of ping backend support, because there’s a trait now, and a mock ping that is used for unit testing \- so although it’s not a swappable backend yet from the user’s perspective, the code is in much better state for this.

Still it does bother me a bit that Surge ping code (the one that does unprivileged ICMP) is not being unit tested at all. Something to look into is that it seems to have a UDP mode, and we could set something up in UDP in localhost to make it work without any privileges.

The collector is creating one socket for connecting to the database per ping target \- that’s right, if there’s 10 targets, there will be 10 connections to the database. A bit weird maybe, but it works pretty well, because each connection identifies itself with the src-dst pair once, and then the DB knows that everything that comes after is for that pair \- so it’s kind of neat.

Also the collector already has database reconnection logic on it. Neat. 

The database... The first thing it strikes as odd is that it opens two different ports, one for collectors and another for GUI. This is a design decision that I don’t like and I am against. I understand that these two usages are very much different but this just looks like lazy design, just to avoid having to deal with a protocol that identifies the role, the wants of whoever calls. And it needs to be solidified later.

Also, no HMAC auth, or roles, or anything at all. The communications it has are very barebones, dumb.

It does save the files already with the src-dst combination in the filename \- but this is not tested.

It does reply to database queries, but just a “give me the last minute” command \- and even this is kind of wrong because it reads from the file, when the database should have a guarantee that all recent data is in memory.

The GUI is a mixed bag. It only does the last minute, and I haven’t run the code to check but in my experience the AI does a bad job at making GUIs that really work.

I think we are missing the CLI here, because that is way more testable, and can be proven to work much more easily. We should probably do a CLI that is as powerful as the GUI, and make the code reusable such that the GUI is a layer on top of the CLI. (or a common client library where both consume from)

There’s also a zzping-tests crate, for integration testing. Currently, this is mainly used to verify that the network communication from the collector works with the database. But in general, this crate should be used to verify that the components do work with each other.

Some honorable mentions of “stuff that we’re missing” is:

* Collectors having an in-memory buffer of hours, that can push to the database on connect.  
* Database being able to receive the collector buffer on connect and deduplicate, or ask the collector to ignore what they already have.  
* Database having a proper in-memory buffer/DB of the recent data, that is used for the real-time part and for queries of recent data.  
* Database pre-loading all indexes and stats that exist on disk, and have them permanently on memory for speeding up queries.  
* Benchmarking and verifying the timing of parallel pings \- thinking and redesigning when the pings should be sent.  
* UDP heartbeat and discovery. That would simplify a lot of configuration.  
* Dynamic config from the database, and telling collectors what to do.  
* Authentication and roles  
* Collector system health communication  
* Zero downtime on updates \- handling this properly. In the new design, it would be because we’d fire two collectors with the same hostname.  
* Still when saving data, we’re missing the YYYY-MM folder format.  
* chunked-v1 likely still is missing storing inside the src hostname and dst target ip.  
* The GUI asks for the last minute, and the last minute is never on disk \- because it should be on memory.

After some consideration, we are moving the networking from serde+bincode to gRPC \+ tonic. We are also moving into TLS, and using standard auth mechanisms instead of HMAC auth.

And all the network interactions are getting complicated, and it needs its own doc. So I will start one just for that analysis. The document [ZZPing - Network protocol (v0.3 concept)](https://docs.google.com/document/d/1tFi44lH-pCbZpxa5VQ01XT8-8e8-b30nyfLAvGVQNdA/edit?tab=t.0) overrides whatever it is said on this document.

