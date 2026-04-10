# Same idea as `perl -lpe '$_=uc'`: chomp each record, mutate $_, print with $\ (output record separator).
$\ = "\n";
while (<DATA>) {
    chomp;
    $_ = uc;
    print;
}
__DATA__
a
b
c
