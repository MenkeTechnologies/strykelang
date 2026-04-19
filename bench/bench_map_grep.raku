my @data = 1..500_000;
my @doubled = @data.map({ $_ * 2 });
my @evens = @doubled.grep({ $_ %% 2 });
say @evens.elems;
