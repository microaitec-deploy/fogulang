"""Standard library reference: archives and Protocol Buffers."""

A = "Archives & serialization"

SECTIONS = [
    {
        "id": "mod-zip",
        "group": A,
        "title": "fogu:zip",
        "import": 'import zip from "fogu:zip"',
        "intro": "ZIP archives (create, extract, list, read, append; AES-256 passwords), tar and tar.gz archives and gzip "
        "data. Extraction refuses entries that would escape the target folder. Binary data comes back as a <a class=\"underline\" href=\"#values\">buffer</a> (inputs also take a str or a list of byte ints).",
        "blocks": [
            {
                "api": [
                    ("zip.zip(dest, source | [sources], { level = 6, store, password })", "int · error", "Folders are added recursively, named like <code>zip -r</code>; returns the number of files."),
                    ("zip.write(dest, { \"name\": str | bytes, \"folder/\": nil }, opts)", "nil · error", "An archive from memory."),
                    ("zip.add(zipPath, name, content, opts)", "nil · error", "Appends to an existing archive."),
                    ("zip.unzip(src, dest = \".\", { password, only: [names] })", "list · error", "The extracted paths."),
                    ("zip.list(src) · zip.read(src, name, { bytes, password })", "list · str | buffer", "<code>[{ name, size, compressedSize, isDir, encrypted, modified }]</code>."),
                    ("zip.tar(dest, sources) · zip.untar(src, dest) · zip.listTar(src)", "", "<code>.tar.gz</code>/<code>.tgz</code> are gzip-compressed."),
                    ("zip.gzip(str | bytes, { level }) · zip.gunzip(bytes, { bytes })", "buffer · str | buffer", ""),
                    ("zip.gzipFile(src, dest?) · zip.gunzipFile(src, dest?)", "str · error", "The written path."),
                ],
                "example": r'''
import zip from "fogu:zip"
import fs from "fogu:fs"
fs.mkdir("site/css")?
fs.writeText("site/index.html", "<h1>hi</h1>")?
fs.writeText("site/css/app.css", "h1 { color: red }")?
print(zip.zip("site.zip", "site")?)
for e in zip.list("site.zip")? { print(e.name, e.size) }
print(zip.read("site.zip", "site/index.html")?)
print(zip.unzip("site.zip", "copy")?)
zip.zip("secret.zip", "site/index.html", { password: "s3cret" })?
print(zip.read("secret.zip", "index.html", { password: "s3cret" })?)
zip.tar("site.tar.gz", "site")?
print(zip.untar("site.tar.gz", "restored")?)
print(zip.gunzip(zip.gzip("hello hello hello"))?)
''',
                "run": True,
            }
        ],
    },
    {
        "id": "mod-protobuf",
        "group": A,
        "title": "fogu:protobuf",
        "import": 'import protobuf from "fogu:protobuf"',
        "intro": "Protocol Buffers without code generation: <code>.proto</code> files are compiled at run time in pure Rust "
        "(protox — no <code>protoc</code>), messages are encoded from and decoded to Fogu maps (prost-reflect). Field "
        "names follow the proto3 JSON mapping (camelCase; <code>protoNames</code> for the original ones); 64-bit integers "
        "are ints; <code>bytes</code> fields are base64 text; well-known types (<code>Timestamp</code>, <code>Duration</code>…) "
        "use their JSON forms.",
        "blocks": [
            {
                "api": [
                    ("protobuf.load(path | [paths], { includes: [folders] })", "ProtoSchema · error", "Imports are searched in <code>includes</code> (default: the files' folders)."),
                    ("protobuf.parse(source, { name, imports: { \"file.proto\": source } })", "ProtoSchema · error", "Google's well-known types are built in."),
                    ("protobuf.loadDescriptor(path | bytes)", "ProtoSchema · error", "A FileDescriptorSet (<code>protoc --descriptor_set_out</code>, <code>buf build</code>)."),
                    ("schema.messages · schema.enums · schema.services", "list", "Services: <code>{ name, methods: [{ name, input, output, clientStreaming, serverStreaming }] }</code>."),
                    ("schema.encode(type, map) · schema.decode(type, bytes, { defaults, protoNames, enumNumbers })", "buffer · map", "Unknown fields and wrong types are error values."),
                    ("schema.toJson(type, bytes) · schema.fromJson(type, json) · schema.validate(type, map)", "", "proto3 JSON."),
                    ("schema.fields(type) · schema.enumValues(enum)", "list · map", "<code>[{ name, jsonName, number, type, repeated, map }]</code>."),
                    ("schema.descriptor() · schema.saveDescriptor(path)", "buffer · nil", ""),
                ],
                "example": r'''
import protobuf from "fogu:protobuf"
const schema = protobuf.parse("
syntax = \"proto3\";
package shop;
enum Status { UNKNOWN = 0; PAID = 1; }
message Item { string sku = 1; int32 qty = 2; double price = 3; }
message Order { int64 id = 1; string customer_name = 2; repeated Item items = 3; Status status = 4; }
")?
print(schema.messages)
const bytes = schema.encode("shop.Order", { id: 42, customerName: "Ana", items: [{ sku: "cafe", qty: 2, price: 4.5 }], status: "PAID" })?
print(len(bytes), "bytes")
print(schema.decode("shop.Order", bytes)?)
print(schema.toJson("shop.Order", bytes)?)
const [_, err] = schema.encode("shop.Item", { nope: 1 })
print(err)
''',
                "run": True,
            }
        ],
    },
]
