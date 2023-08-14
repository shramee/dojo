POST http://0.0.0.0:5050
Content-Type: application/json
{
    "jsonrpc": "2.0",
    "method": "katana_getStorageAtMultiple",
    "params": [
        [
            [
                "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7",
                [
                    "0x37c96558f481f70f2c8b6c29e2d05668a3ec0b30c56618a59c16edd587bfc44",
                    "0x30c56618a59c16edd587bfc4437c96558f481f70f2c8b6c29e2d05668a3ec0b"
                ]
            ],
            [
                "0x041b1562049d36570d4e46f4b82f9e004dc70599674bd3fcc84644ddd6b96f7c",
                [
                    "0x0f70f2c8b6c29e2d05668a3ec0b30c56618a537c96558f489c16edd587bfc44",
                    "0x016edd587bfc4437c96558f481f70f2c8b6c230c56618a599e2d05668a3ec0b"
                ]
            ]
        ]
    ],
    "id":1
}

HTTP 200
[Asserts]
jsonpath "$.error" not exists
jsonpath "$.result" exists
jsonpath "$.result[1][1]" exists
