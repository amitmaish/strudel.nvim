lua/strudelserver.so: $(wildcard src/*) strudel-frontend/dist/index.html
	cargo build --release
	cp -f target/release/libstrudel.dylib lua/strudelserver.so

strudel-frontend/dist/index.html: $(wildcard strudel-frontend/*)
	( cd strudel-frontend/; npm run build )

.PHONEY: clean
clean:
	cargo clean
	rm lua/strudelserver.so

.PHONEY: webdev
webdev:
	( cd strudel-frontend/; npm run dev )
