#!/usr/bin/env python

import sys
import asyncio
import ssl
from websockets.sync.client import connect

def send():
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    with connect("wss://192.168.1.199:2000", ssl_context=context) as websocket:
        file = open(sys.argv[1])
        lines = file.readlines()
        file.close()

        for line in lines:
            websocket.send(line)
        input()

send()

