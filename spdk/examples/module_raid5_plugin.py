# This is a plugin for the SPDK's rpc.py JSON-RPC client that enables it to call into the `module_raid5.rs`
# example's RPC calls.
#
# To use it, add the path to the spdk crate's `examples` directory to the `PYTHONPATH` environment
# variable and specifiy the basename of this file to the `--plugin` option of `rpc.py`, e.g.
#
# $ PYTHONPATH="${PYTHONPATH}:${RUST_SPDK}/spdk/examples" "${SPDK}/scripts/rpc.py" \
#   --plugin module_raid5_plugin ...
#
import re

from functools import partial
from spdk.rpc.cmd_parser import print_json

UNITS = {None: 1, "KiB": 2**10, "MiB": 2**20, "GiB": 2**30, "TiB": 2**40}

def parse_size(size):
    m = re.match(r'^([\d|\.]+)\s*((?:K|M|G|T)iB)?', str(size).strip())
    if m:
        num, unit = m.groups()
        return int(float(num) * UNITS[unit])
    raise ValueError("Invalid size specified")

def spdk_rpc_plugin_initialize(subparsers):
    def create_raid5rs_bdev(args):
        print_json(args.client.create_raid5rs_bdev(name=args.name,
                                                   stripe_size=args.stripe_size,
                                                   children=args.children))

    p = subparsers.add_parser('create_raid5rs_bdev',
                              help='Creates a Rust implementation of a RAID-5 block device.')
    p.add_argument('name', help='The name of the RAID-5 block device.')
    p.add_argument('-s',
                   '--stripe-size',
                   help="The stripe size, in bytes. This can be a bare number (e.g. 65536) or a number "
                       +"with unit suffix (e.g. 64KiB).",
                   type=parse_size,
                   required=True)
    p.add_argument('-c',
                   '--children',
                   help="A comma-separated list of child block device names.",
                   type=partial(str.split, sep=','),
                   required=True)
    p.set_defaults(func=create_raid5rs_bdev)

    def destroy_raid5rs_bdev(args):
        print_json(args.client.destroy_raid5rs_bdev(name=args.name))

    p = subparsers.add_parser('destroy_raid5rs_bdev', help='Creates a Rust implementation of a RAID-5 block device.')
    p.add_argument('name',
                   help='The name of the RAID-5 block device.')
    p.set_defaults(func=destroy_raid5rs_bdev)
