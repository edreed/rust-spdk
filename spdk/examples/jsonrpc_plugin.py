# This is a plugin for the SPDK's rpc.py JSON-RPC client that enables it to call into the `jsonrpc.rs`
# example's RPC calls.
#
# To use it, add the path to the spdk crate's `examples` directory to the `PYTHONPATH` environment
# variable and specifiy the basename of this file to the `--plugin` option of `rpc.py`, e.g.
#
# $ PYTHONPATH="${PYTHONPATH}:${RUST_SPDK}/spdk/examples" "${SPDK}/scripts/rpc.py" \
# --plugin jsonrpc_plugin hello_world
#
from spdk.rpc.cmd_parser import print_json

def spdk_rpc_plugin_initialize(subparsers):
    def hello_world(args):
        print_json(args.client.hello_world())

    p = subparsers.add_parser('hello_world', help='Prints a gretting to the world')
    p.set_defaults(func=hello_world)

    def print_message(args):
        print_json(args.client.print_message(msg=args.msg))

    p = subparsers.add_parser('print_message', help='Prints the specified message to stdout')
    p.add_argument('msg', help='The message to print')
    p.set_defaults(func=print_message)

    def add_two(args):
        print_json(args.client.add_two(a=args.a, b=args.b))

    p = subparsers.add_parser('add_two', help='Adds two integers and returns the result')
    p.add_argument('a', help='The first number', type=int)
    p.add_argument('b', help='The second number', type=int)
    p.set_defaults(func=add_two)

    def guess_number(args):
        print_json(args.client.guess_number(guess=args.guess))

    p = subparsers.add_parser('guess_number', help='Guess a number between 1 and 10')
    p.add_argument('guess', help='A number between 1 and 10, inclusive', type=int)
    p.set_defaults(func=guess_number)

    def write_file(args):
        print_json(args.client.write_file(filename=args.filename,text=args.text))

    p = subparsers.add_parser('write_file', help='Write text to a file')
    p.add_argument('filename', help='The file to write')
    p.add_argument('text', help='The text to write')
    p.set_defaults(func=write_file)
