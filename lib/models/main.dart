part 'main.flu.dart';

// @flu #x copyWith=false #ano
abstract class _User {
  // @flu enum
  Role? get x;
}

enum Role {
  admin('A'),
  user('B');

  const Role(this.apiKey);
  final String apiKey;
}
