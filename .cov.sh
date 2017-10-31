#!/bin/sh

sleep 20
echo "Running protocol tests"
python ./protocol/sockjs-protocol.py
curl -v http://127.0.0.1:52081/exit.html
sleep 5
