# zzping-lib

Set of common code to read and write messages that contain statistics of the 
pings performed.

This folder contains a set of tools to parse and convert files:

* dataread: Obsolete experimental program to convert files to a compressed form.
  DO NOT USE.
* datareadq: Current program used to convert files from zzping-daemon output 
  format (FrameData) into a file format that the GUI can read (FrameDataQ).
* fdqread: Utility to read FrameDataQ files and stitch them and recompress them.
  Also it can be used to output the contents to stdout in clear form.

## DataReadQ Utility

Reads data logged by zzping-daemon and converts it to FrameDataQ, storing it on
disk.

Basic Options:
  * input: List, space separated, of input files to read.
  * output: File to output data to. If several inputs are given and this output
    is set, it will join all files into one output respecting the ordering given
    on the input flag. Omitted when auto-output is used.
  * auto_output: If passed, it will guess a output filename based on the input
    name. This option will create a file for each input file given.

Compression Options:
  * quantize: If passed, enables quantization. Losses precision but files may be
    smaller. See bellow on FrameDataQ format to see sensible values.
  * time: How often to write a full frame. 60s by default.
  * delta-enc: If passed, delta encoding is used. Currently buggy and the 
    resulting files might not be readable.

## FdqRead Utility

Reads FrameDataQ files and can conbine several into one single output, recompress
them, or dump them to stdout. 

It also has the ability to aggregate, this is, to extract percentiles from N 
frames and dump a single frame. This is useful when joining several days worth
of data, so zzping-gui can load quicker and work with sets that contain weeks.

This can be good for long-term archiving. When the data is old, we can choose to
reduce the timing resolution and just leave the overall timings (per second,
per minute).

Basic Options:
  * input: List of files to process, space separated. Will be joined on the output.
  * output: Output file to save. If omitted, dumps text to stdout.

Compression Options:
  * quantize: If passed, enables quantization. Losses precision but files may be
    smaller. See bellow on FrameDataQ format to see sensible values.
  * time: How often to write a full frame. 60s by default.
  * delta-enc: If passed, delta encoding is used. Currently buggy and the 
    resulting files might not be readable.

Aggregation Options:
  * agg-step: Reduces the output timing resolution by this factor. If we pass
    10, it will create one frame out of every 10.
  * agg-window: How many samples to aggregate into one. Must be at equal or 
    bigger than agg-step. This is used to smooth out values.

## Definitions

### Frame

In this project, a "frame" is defined to be a block of time that happens between
reporting intervals.

This is, if the daemon reports every 100ms, then outputs a frame every 100ms.

Frames can contain zero, one, or many pings; depends on the protocol and 
configuration used.

## Formats

Most/all formats here use MessagePack for encoding. This greatly simplifies
encoding/decoding, but be aware that MessagePack is not used here in the common 
sense. Usually, one would expect that decoding a MessagePack entity would 
retrieve either the whole file, a block or at least a single frame.

This is not the case here. These formats encode multiple symbols per frame.
The reason for this is space efficiency. This tool can create millions of 
records in a short period of time, and being able to represent them efficiently
on disk or on network is critical.

So, if you expect to see something like:

```python
>>> data = msgpack.unpackb(raw_file, raw=False)
>>> repr(data)
{"ipaddr":"9.9.9.9", "when":"2021-01-01", "time_us":15000}
```

You'll be quite disappointed, because in fact, you'll get something like:

```python
>>> data = msgpack.unpackb(raw_file, raw=False)
>>> repr(data)
1532
```

In order to read the file, it's imperative to continue reading several times
ahead, and compose the messages as you go. `framedata_reader_test.py` in this 
folder has an example on how to parse FrameData messages in a self-contained way.

### FrameStats (UDP)

R/W Library: src/framestats.rs

This format is used to encode the statistics printed on the CLI of the daemon
into a packet that then it can be sent via UDP to the GUI.

Each message is written as an array of 5 elements:

```python
[
    address_string,  # str, Target Host Address.
    inflight_count,  # u16, Amount of packets awaiting for response at this time.
    avg_time_us,     # u32, Mean time in microseconds to reply on the packets returned on this frame.
    last_packet_ms,  # u32, Time in milliseconds of how long since the last reply was heard.
    packet_loss,     # u32, Packet loss percent * 1000.
]
```

At the time of writing this, this format is the only one that can be easily
parsed.

This format does not contain timestamps, and therefore if stored on disk "as-is"
it becomes a bit useless.

### FrameData (Disk logging for daemon)

R/W Library: src/framedata.rs

This is the format used by zzping-daemon to store the pings on disk. As it is a
bit bulky, it is going to be phased out in the future.

This format is a bit tricky to parse, as not only requires reading multiple
symbols per frame, also the amount of symbols can be either 4 or 5. The parser
needs to be a bit smart to parse this properly.

If the first symbol is a string, this will require an extra symbol. If it is a
u32, then it's a regular message.

The message consists of these parts:

* Time: 1 or 2 symbols.
  * Full encoding:
    * timestamp: str, timestamp for this frame in rfc3339 with microsecond precision.
    * delta: u32, always zero in this case.
  * Delta encoding:
    * delta: u32, number of microseconds passed since the last timestamp written.
* inflight: u16, number of packets waiting to be received.
* lost_packets: u16, number of packets detected as lost in this frame. 
  (They might have been sent in previous frames)
  NOTE: This metric might be broken.
* recv_packets: An array of 0-N elements
  * element: u32, microseconds waited to receive the ping. There is one element
    for each ping received; Order does not matter, but usually is written sorted 
    by timing.

NOTE: The target IP Address is not stored in this format. It should be on the
filename.

### FrameDataQ (disk logging for GUI)

R/W Library: src/framedataq.rs

This is the new format proposed which compresses the data further and uses less
space by sacrificing some precision. It has some pluggability for compressors, 
but the only one supported is the LinearLogQuantizer, which reduces the space
used by storing only the first significant numbers of the timings.

It is intended for disk storage, although it could be used later for network
communication.

At the time of writting this, the binaries that write this format are `datareadq`
and `fdqread`. The binaries that can read this are `fdqread` and `zzping-gui`.

NOTE: The target IP Address is not stored in this format. It should be on the
filename.

### FrameDataQ Header

As the format is configurable, and different configurations are incompatible
between each other, a header is written on the beginning of each file, in order
to know how to decode it properly. Otherwise any update on the configuration
would render the files unreadable.

The format of the header is a single map symbol that contains:

```python
{
  "schema": HEADER_SCHEMA, # str, should be "FDCodec". Used to verify that the file contains the desired format.
  "version": HEADER_VERSION, # uint, should be 101. Version used to encode this file.
  ... other data here ...
}
```

The schema just ensures that the data stored is expected, that this is not just
another random file.

Version controls what other data can be found in the header, as well to enable
certain features of the encoder/decoder. If a feature is disabled, it will have
a compatible default. This is done to ensure backward compatibility.

Currently there is no way to set the version when encoding. It's always the
latest version. This might cause some problem if we want to decode data in an
earlier version, as we cannot set to which target version to encode.

For version 101, the extra data in the header is as follows:

```python
{
  "schema": HEADER_SCHEMA, 
  "version": HEADER_VERSION,

  "full_encode_secs": 60,  # uint, How many seconds between full encode frames.
  "recv_llq": None,        # Option<f64>, activates LinearLogQuantizer.
  "delta_enc": False,      # bool, activates delta encoding.
}
```

This format reduces the size required by encoding delta times as in FrameData
every N frames. full_encode_secs sets the limit for when to encode the time 
from an absolute value again.

recv_llq enables quantizing. This means that the timings will lose precision but
they might fit into tighter types. Some example values are:

* 0.001: 0.1% loss, the value encoded is always within a 0.1% margin of the
  original.
* 0.01: 1% loss
* 0.1: 10% loss
* 1.0: 100% loss. The value encoded for this case is log2(orig).

delta_enc is broken for now. Activating it will yield wrong values when decoding.
The idea is to store a delta between the different ping times in a frame, but
it seems to have a bug.

The internals of each frame are encoded quite similar to FrameData, with the
exception that we store 7 percentiles instead of a variable size array of ping
timings.

## BatchData (Experimental unused format)

This format was the first attempt to get better compression ratios from 
FrameData by packing several frames together.

After a lot of tries and work, it turns out that a simpler approach like in
FrameDataQ packs the data almost as efficiently, making this format too complex
and expensive to decode.

I might come back later to this and get into a better packing format. For now it
is just experimental and doesn't perform any beter (or marginally better).
