# Subtitle fixtures

Binary subtitle files for `MPVLibPlaybackTest.bitmapAndMp4TextSubtitlesAreDecoded`. Each holds
one subtitle from 0 s; the test loads it as an external subtitle under a WAV it generates.
Made with FFmpeg 9.0.2 and Python 3.12; regenerate them the same way if a format needs to change.

| File | Format | How it was made |
|---|---|---|
| `pgs.sup` | HDMV PGS (Blu-ray), `hdmv_pgs_subtitle` | By hand, because FFmpeg has no PGS encoder: `python -I make_pgs.py pgs.sup`. One display set (PCS, WDS, PDS, ODS, END) shows a 64x16 white bar at 0 s, a second (PCS without objects, WDS, END) clears it at 10 s. `ffmpeg -i pgs.sup -map 0 -c:s dvdsub -f vob -y /dev/null` decodes both display sets without error. |
| `vobsub.idx`, `vobsub.sub` | DVD (VobSub), `dvd_subtitle` | `vobsub.sub` from `pgs.sup`, decoded and encoded again by FFmpeg (bitmap to bitmap): `ffmpeg -i pgs.sup -map 0 -c:s dvdsub -muxpreload 0 -muxdelay 0 -f vob vobsub.sub`. `vobsub.idx` written by hand (FFmpeg has no VobSub muxer): the 720x480 size, FFmpeg's default dvdsub palette, and one `timestamp: 00:00:00:000, filepos: 000000000` line for stream `en`. FFmpeg's encoder drops the clearing display set, so the subtitle's own stop delay is the maximum (745.6 s). |
| `mov_text.mp4` | MP4 timed text, `mov_text` | From a one-line SubRip file (`00:00:00,000 --> 00:00:10,000`, `Hello mov_text`): `ffmpeg -i in.srt -map 0 -c:s mov_text -map_metadata -1 -fflags +bitexact -flags:s +bitexact -f mp4 mov_text.mp4`. |
