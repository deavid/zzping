
# Storing history

We could:

1) Store individual ping responses + ping failures
2) Store metrics / statistics

Storing individual pings cons:

* It is going to use a LOT of space on disk. With 100+ req/s, we can fill any
  disk quite easily.

Storing statistics has the problems:

* If any other analysis outside of the current statistics is wanted later, it is
  lost.

# Individual ping space used

```
    pub seqn: u16,          (2 bytes)
    pub ident: u16,         (2 bytes)
    pub addr: IpAddr,       (16 bytes) IPv6 support!
    pub sent: Instant,      (8 bytes)
    pub when: SystemTime,   (8 bytes)
    pub received: Option<Duration>,  (8 bytes + NULL)
```

We could use either negative or zero values for NULL in received.
This would make 44 bytes per ping.
At 200 req/s this is 8800B/s (8.6KiB/s, 21.5GiB/month) 

For how long we do want to hold this data? 
Ideally one month, or if not, one week.

## Tricks to compress this data down

Values `ident` and `addr` will not change for a stream. If we store these per
file, or every X time, these are not needed per packet.

`sent` is worthless as it is not system time.

`seqn` and `when` are always incrementing, so we could use some kind of diff
approach. 

* ATTENTION: `seqn` *does* wrap over!
* ATTENTION: `when` is actually a full time. A diff could make it totally 
  unreadable!
* ATTENTION: `when` can actually go backwards in time!

`received` could have less precision. We could use logarithm or sqrt to make the
precision relative to the amount.

A diff `when` could also have less precision in the same fashion as received.
But we need to be able to handle negative values.

We need a mode to output a full, uncompressed packet, when compression is 
unreliable, or at least every X values.

`received` has to range from 0.01ms to 10s. We can use 32bits / 4 bytes.

In the end:

* addr: 16bytes / ~1000 req. ~0bytes / req
* ident: 2bytes / ~1000 req. ~0bytes / req
* sent: 0, removed.
* seqn: 2bytes (w/o diff)
* when: 4bytes w/ diff, 8bytes / ~1000 req.
* received: 4 bytes.

Total: 2+4+4 = 10 bytes / req. 4.4x compression.

This moves us from 21.5GiB/month to 4.87GiB/month.

## Re-packing data

All in all, we cannot stream individual packets into disk, because unless they
have been replied to, we need to wait at least 10s to discard them.

If we process this in chunks, we can probably compress this even further.

Creating "packets" of 10s each (or 20k req), we could have:

* Header: 92 bytes
  * Size/req count: 4 bytes (32b)
  * 1st packet data: 28 bytes
    * addr 16 bytes
    * ident 2 bytes
    * seqn 2 bytes
    * when 8 bytes
  * Statistics: 80 bytes
    * Percentiles (5, 7 or 9) of received timings: 4 bytes * 10 = 40 bytes
        * we could define by:
        * apply cubic square to the value. (to lose precision on high values)
        * 1st percentile, min (total value)
        * next percentiles, as multiplier of the previous. (1.001x as minimum)
    * Percentiles of `when`, as the difference from expected value: 4 bytes * 10 = 40 bytes
        * `when` max value
            * `when` can go backwards. We can use `sent` to calculate the max.
            * or instead, just store as elapsed time using `sent`
        * assume equally spaced
        * compute the difference from perfect spacing
        * retrieve percentiles as before, linear.
* Data (x20k): 2 bytes * 20k
  * These values map as 0-N interpolating from percentiles.
  * sorted by `sent` or `seqn` (sent is better as does not wrap over)
  * when_diff: 6bits. linear, 64 values.
  * received: 10bits. cubic, 1023+1 values. 
    * 0 value means not received.
    * missing seqn are not expected here. (does it matter this value?)

Total: 92 + 2 * 20000 = 40092; 2.0046 / req

> 2.0046byte * 200/s * 1 month to MiByte
>
>  (((2,0046 * byte) * 200) / second) * (1 * month) = approx. 1005,4964 mebibytes

Around 1 GiB/month, 33MiB/day. 23.5KiB/min.

Instead of using different files, we can just store one packet per stream, one
after the other.

### Data alignment

For the CPU's sake, we should align the data at 8 byte boundary.

This means that the header needs to be 96 bytes (+4 byte).

The next packet needs to be written at the next boundary, so extra padding would
be needed, depending on the size. (We can take this into account to make the 
size an actual multiple of 8 bytes)

## Indexing

Storing all the data packed together is nice, but it is impossible to search.

We need to be able to:
* Go to a particular point in time **fast**.
* Be able to render a zoom out picture **fast**.

While the data files can be splitted by UTC day or UTC ISO week (this would be 
hard to get the month from the disk filename), going inside to a particular point
in time will be costly.

Disk is read at 4KiB chunks, just reading the header and skipping would take
really long, as one entry is at most 10 chunks and disks don't work well in 
random read scenarios.

We also want all files to be written left-to-right, no random writes, so 
repacking or adding info in the base file is out of the question. This is to
avoid data loss scenarios like power outages.

An index can, for example, contain just the basic header info:

* req count/packet size in bytes: 4 bytes
* when: 8 bytes

Storing this for each packet is still not efficient, as we have 1 packet per 
target host. We would need to store it once per X amount of time.

Also, this data is not 8 byte aligned. We could do a bit better by doing:

* position in 8byte multiples: 4 bytes. (up to 34GiB per file)
* max_when: 4 bytes (we can leverage that the day is known, just store time)

If we store 1 idx every 10s, then we get 67.5KiB/day.

This is quite fast to read and we can locate easily where to start reading.

## Aggregation

In last section we only solved how to position quickly. But a really zoomed-out
view that covered a day (or many days) would still have to parse a lot of data.

Reading from disk might be fast, but parsing it and computing aggregates is 
going to be really slow. This needs to be pre-stored.

### Frequency

How often should we save aggregated values? 1/sec?

To display up to 1 day, 86400 entries is quick to process.

To display 1 month, there will be 2.6M entries.

On the other hand, if the resolution is just above than 1/sec, we could have
a window around 4000px wide (4k), and therefore we will need to process 4000s
of data. 4000 * 200 = 800k packets.

We could do 10/sec, and then for the full day is 864k, and for the zoomed in is
around 80k entries.

This would mean that we need a second aggregation step (we needed it anyway).

1/minute would be 600 times less. Seems about right. 1440 entries per day, which
for most window widhts would fit in screen. This is 43830 entries per month.

This could mean 1 file per day, and another per month, each one containing its
own aggregation resolution.

### Response times

We can just use the same percentiles, as they are easy to aggregate and a mean
can be also calculated from them if needed. They provide most of the info needed.

### Packet loss

The problem with packet loss is that just counting packets or % loss doesn't
give the full picture, specially when seen from hours/daily perspective.

Is it a 30% constant loss? or just a 100% spike for seconds/minutes?

Packet loss cannot use percentiles because technically there are only two 
possible values: 0% or 100%.

The simple way of doing this is to compute percentiles over an runnig average,
but then the problem is, what width we should use to do this average? Different
values are going to return completely different values.

Averages that are too short are going to fall always on 0 an 100%, and the 
percentile is not going to tell us much.

Averages that are too long will mask continuous 100% packet loss into lower 
tiers.

Counting the number of consecutive seconds of packet loss and/or packet received
could give a better idea, but I'm unsure on how to parse/draw that later.

We need to define what is "too long" for packet loss. For example, if no packet
succeeded in 5-10s, that is for sure a connection loss. TCP can handle higher
losses, such as 5 minutes, but 5 seconds is already noticeable by the user.

A packet loss% over 1 second could be a really good measure, with the only 
problem of not knowing if over that second for example 30% of probes failed 
randomly or they were packed together.

Therefore we could add a packet loss mean duration metric, this could range from
1 frame (<1ms) to 1 second. We can use percentiles on this, and it can show if
the problem was random drops or continuous.

NOTE: The regular packet latency time is 10/s without rolling windows. This one
should be as well. Can we update this method so it goes in sync?

In this case the average would be over those 100ms, and the duration metric 
would range up to 100ms.

Can this data be further aggregated later, say for semi-zoom out or for monthly?

Yes, the averages can be added together to create new points or converted into
percentiles. The loss duration metric can be aggregated into percentiles.

NOTE: The averages require high precision on the lower values! Consider storing
logarithm or cubic roots instead.

Packet loss is in fact newly created data at this step, it is not really an 
aggregation which means that further aggregation would change the data format.

NOTE: Packet loss is hard to place in time. It can happen that a packet was
received after a packet was lost, but we are not taking into account the 
potential response time of that lost packet because we lack that information. We
could assume a respose time being similar to the average or median of the 
target.

### Indexing on aggregation files

These files can be long, up to one million entries. How do we look up particular
points in time?

The regular indexing file for the day could hold values as well for positions
in the aggregation file. Every minute would be more than enough.

Separate indexing files might also be just easier and faster.

### Storing format for aggregated values

We need to store:
* Response time
  * Percentile points  
    (We need to aggregate them down but keep high resolution)
    (256 points?)
  * Percentile count
    (As we aggregate them down, we would have more than 1 point, so count is needed)
* Average Packet loss

# Not storing history, only aggregates

## Motivation

Sending raw packets to the UI just moves the problem from one program
to the next. The aggregates are all we need as they contain already everything
we need in a 10/s resolution.

## Problem

The aggregation was expected to run at 10 seconds after the packets were 
received. If we want to just send this to the UI in real-time, the information
is not there yet.

We could aggreate as we receive instead of as we send, but this is not accurate.
Packets could arrive highly out of order and we counted where they don't belong.
(Actually, the problem, whatever it was, happened between sent and received...)

We could do a 2-pass option, one for real-time, and another after a few seconds
for accurate results. For this to work we would need to have IDs/fingerprint the
times so we can accurately replace them on the UI.

Hosts that reply in 80-150ms range (internet) would also cause a problem, so it
makes more sense to display them in UI as they're received.

But this would shift these results one tick behind as we do the 2nd pass.

The big problem is a different beast here: packet loss. We cannot declare a 
packet to be lost within a 100ms timeframe (Ideally in less than 20ms to fit on
the end of the window). 

To declare a packet lost, at least temporarily, it should at the very least 
double the latency of the 50th percentile.

Another way of finding or guessing packet loss is the lack of packets received,
for example comparing the amount sent per second with the amount received.

If we usually send 40req/s but we received 20 in the timeframe, a good guess is
50% packet loss.

Potentially, this method is barely the same as 2x 50th percentile guessing, just
it shifts the data forward in time, but it should be more or less the same.

What this tells us is that the UI should be able to update constantly 2-3 ticks
behind. It should be fine; the user would see these lines a few pixels behind,
which in fact, it's completely true.

Then this means we need a new category of data for packets potentially lost;
Either that or just report them as lost and they will "come back" later.

# Aggregation Data format

* TimeUID: u32 - counter reset by day, frame number, shared across different targets
* Target: (18bytes)
    * Address: 16bytes, 128bits
    * ident: u16
* Response time histogram (40 bytes)
    * list of (up to) 10 entries, sorted: 
        * response time in microseconds: u32
* Packet loss duration histogram (40 bytes)
    * list of (up to) 10 entries, sorted:
        * consecutive packet loss duration in microseconds: u32
* Average packet loss percent (4 bytes)
    * list of (normally one) entry, sorted:
        * average packet loss percent, times 1000: u32

This would make: 106 bytes per frame per host.
Assuming 10 hosts, this is 1060 bytes/frame, or 10.35KiB/s.

For sending data across the network this is perfectly fine.

For storing, this is 873.4MiB/day. Storing this, even overwritting files, is out
of the question. This process will make disks degrade, specially for SSD.

When storing, it would be better storing around 100 frames at once (10s). We
could leverage this to compress the data better.

For zooming purposes, the ideal would be coverting 8 frames into one, and then 
again recursively, so it goes: 8, 64, 512, 4096 (0.8s, 6.4s, 51.2s, 6 min 50s)

And use the zoom from above to reduce information on the zoom below it. (Is it
actually possible, and would it reduce data?, 512 and 4096 are probably going
to display similar information, therefore we could reduce the 512 space, but
the effect of the big zoom levels on the actual data usage below is going to be
minimal)

* TimeUID: u32 4B - counter reset by day, frame number, shared across different targets
* Time: (12 bytes)
  * Start time: 8 bytes, 64bits
  * Average interval: 4 bytes, 32bits
* Target list: (assume 10 targets)
  * Target: (20 bytes)
    * Address: 16bytes, 128bits
    * ident: u16, 4 bytes
  * Stats: 8 bytes
    * Minimum latency us: u32
    * Max latency us: u32
  * Time tick: (10 ticks, 1 second) (10 bytes * 10 = 100 bytes)
    * Response time histogram (6 bytes)
      * list of 3 entries, sorted: 
        * 50%, 70%, 90%
        * u16, compressed
    * Packet loss duration histogram (3 bytes)
      * list of 3 entries, sorted: 
        * 50%, 70%, 90%
        * consecutive packet loss duration in milliseconds: u8
    * Average packet loss percent (1 byte)
      * list of (normally one) entry, sorted:
        * average packet loss percent, times 1000: u8
Total: 4+12+(20+8+100)*10= 1280+16 = 1296B/s ; 107MiB/day.

For 10 seconds:
Total: 4+12+(20+8+1000)*10= 10296B/10s = 85MiB/day.

Maybe I'm too worried about SSD wearing; it seems most of them could hold 
10GiB/day for 25 years. 

The only problem might be writting under 4kB, because we might end writting the
same sector several times. OS buffering should help here.
