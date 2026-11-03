import zlib
HDR = b'\x1f\x8b\x08'
def rows(path):
    """Every line of a file of concatenated gzip members, some of them truncated (a run killed before its finish):
    a broken member yields what decodes, then the reading resumes at the next gzip header."""
    if not path.endswith('.gz'):
        for line in open(path, encoding='utf-8', errors='replace'):
            yield line.rstrip().split(',')
        return
    data = open(path, 'rb').read()
    pos = 0
    CH = 1 << 20
    while True:
        pos = data.find(HDR, pos)
        if pos < 0:
            return
        d = zlib.decompressobj(16 + zlib.MAX_WBITS)
        i = pos; rest = b''; ok = False
        while i < len(data):
            try:
                o = d.decompress(data[i:i + CH])
            except zlib.error:
                break
            ok = True
            lines = (rest + o).split(b'\n'); rest = lines.pop()
            for line in lines:
                yield line.decode('utf-8', 'replace').split(',')
            if d.eof:
                i = len(data) - len(d.unused_data) if d.unused_data else i + CH
                break
            i += CH
        pos = i if ok and i > pos else pos + 1
