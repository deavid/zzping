# Copyright 2020 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

import msgpack
import sys
import re
from datetime import datetime
from datetime import timedelta


class Stats:
    timestamp: str
    elapsed: int
    inflight: int
    lost: int
    received: list

    def __init__(self, items: list, last_timestamp=None):
        if isinstance(items[0], str):
            timestamp = items.pop(0)
            self.timestamp = datetime.fromisoformat(timestamp)
        else:
            self.timestamp = last_timestamp
        try:
            self.elapsed, self.inflight, self.lost, self.received = items
        except ValueError:
            raise StopIteration
        self.elapsed = timedelta(microseconds=self.elapsed)

    def __repr__(self):
        timestamp = "None"
        if self.timestamp:
            timestamp = self.timestamp + self.elapsed
            timestamp = timestamp.strftime("%H:%M:%S.%f")

        return "<Stats: %s i:%d l:%d r:%r>" % (
            timestamp,
            self.inflight,
            self.lost,
            self.received,
        )


class Reader:
    def __init__(self, filename):
        buf = open(filename, "rb")
        self.unpacker = msgpack.Unpacker(buf, raw=False)
        self.last_timestamp = None

    def __iter__(self):
        while True:
            try:
                yield self.parse()
            except StopIteration:
                break

    def parse(self):
        timestamp = None
        items = []
        for unpacked in self.unpacker:
            items.append(unpacked)
            if isinstance(unpacked, list):
                break
        if len(items) == 0:
            raise StopIteration
        ret = Stats(items, self.last_timestamp)
        self.last_timestamp = ret.timestamp
        return ret


filename = sys.argv[1]
reader = Reader(filename)

for n, elem in enumerate(reader):
    print(elem)
    # if n > 10000:
    #     break
