FROM busybox
CMD ["sh", "-c", "echo 'hello from tharhosting' > index.html && httpd -f -p $PORT"]
