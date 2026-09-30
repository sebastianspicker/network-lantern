# Bounded asynchronous native-stream reader (private to NetworkLantern.Throughput)

# Add-Type types live for the PowerShell process lifetime. Re-importing the
# module must reuse the reader rather than attempting to define it again.
if (-not ('NetworkLantern.Throughput.Native.BoundedStreamReader' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Threading.Tasks;

namespace NetworkLantern.Throughput.Native
{
    public sealed class BoundedStreamReadResult
    {
        public byte[] Bytes { get; set; }
        public long TotalBytes { get; set; }
        public bool Truncated { get { return TotalBytes > Bytes.LongLength; } }
    }

    public static class BoundedStreamReader
    {
        public static async Task<BoundedStreamReadResult> ReadAsync(Stream stream, int maxBytes)
        {
            if (stream == null) throw new ArgumentNullException(nameof(stream));
            if (maxBytes < 1) throw new ArgumentOutOfRangeException(nameof(maxBytes));

            byte[] readBuffer = new byte[81920];
            byte[] captured = new byte[maxBytes];
            int capturedCount = 0;
            long totalBytes = 0;
            int read;
            while ((read = await stream.ReadAsync(readBuffer, 0, readBuffer.Length).ConfigureAwait(false)) > 0)
            {
                int remaining = maxBytes - capturedCount;
                if (remaining > 0)
                {
                    int copyCount = Math.Min(remaining, read);
                    Buffer.BlockCopy(readBuffer, 0, captured, capturedCount, copyCount);
                    capturedCount += copyCount;
                }
                totalBytes += read;
            }

            if (capturedCount != captured.Length)
            {
                Array.Resize(ref captured, capturedCount);
            }
            return new BoundedStreamReadResult { Bytes = captured, TotalBytes = totalBytes };
        }
    }
}
'@
}
