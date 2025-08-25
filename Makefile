lua/strudelserver.so: $(wildcard src/*)
	cargo build --release
	cp -f target/release/libstrudel.dylib lua/strudelserver.so

.PHONEY: clean

clean:
	cargo clean
	rm lua/strudelserver.so
