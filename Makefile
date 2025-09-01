lua/strudelserver.so: target/release/libstrudel.dylib 
	cp -f target/release/libstrudel.dylib lua/strudelserver.so

target/release/libstrudel.dylib: $(wildcard src/*) strudel-frontend/dist/index.html
	cargo build --release

strudel-frontend/dist/index.html: $(wildcard strudel-frontend/*)
	( cd strudel-frontend/; npm run build )

.PHONEY: clean
clean:
	cargo clean
	rm lua/strudelserver.so

.PHONEY: webdev
webdev:
	( cd strudel-frontend/; npm run dev )
